//! Local captcha OCR. The ONNX model and charset are embedded so the binary is
//! self-contained; `captcha-ocr` controls whether they are compiled in.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreprocessMode {
    /// Scale to height 64, keep aspect ratio, ITU-R 601 luma.
    Baseline,
    /// Crop to the text bounding box, add a 4px white margin, then scale.
    BoundingBoxCrop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrOutput {
    pub text: String,
    pub mode: PreprocessMode,
}

/// Decode `data:image/...;base64,...` into raw bytes.
pub fn decode_data_uri(uri: &str) -> Result<Vec<u8>, String> {
    let value = uri
        .split_once(',')
        .map(|(_, value)| value)
        .ok_or("验证码图片不是 data URI")?;
    B64.decode(value.trim())
        .map_err(|error| format!("验证码 base64 解码失败: {error}"))
}

/// Try baseline first, then the bounding-box crop when the caller has an
/// expected length and the first pass disagrees.
pub fn recognize(image_bytes: &[u8], expected_length: Option<usize>) -> Result<OcrOutput, String> {
    let baseline = recognize_with(image_bytes, PreprocessMode::Baseline)?;
    let baseline_ok = expected_length
        .map(|expected| baseline.chars().count() == expected)
        .unwrap_or(true);
    if baseline_ok {
        return Ok(OcrOutput {
            text: baseline,
            mode: PreprocessMode::Baseline,
        });
    }
    let cropped = recognize_with(image_bytes, PreprocessMode::BoundingBoxCrop)?;
    Ok(OcrOutput {
        text: cropped,
        mode: PreprocessMode::BoundingBoxCrop,
    })
}

#[cfg(feature = "captcha-ocr")]
mod engine {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
    use ort::session::Session;
    use ort::value::Tensor;

    static MODEL_BYTES: &[u8] = include_bytes!("../../assets/captcha-common_old.onnx");
    static CHARSET_RAW: &str = include_str!("../../assets/captcha-charset.txt");

    pub struct OcrEngine {
        session: Session,
        /// 0 = CTC blank, because the charset file starts with an empty line.
        charset: Vec<&'static str>,
    }

    impl OcrEngine {
        pub fn load() -> Result<Self, String> {
            let session = Session::builder()
                .and_then(|builder| builder.commit_from_memory(MODEL_BYTES))
                .map_err(|error| format!("验证码模型加载失败: {error}"))?;
            let charset: Vec<&str> = CHARSET_RAW.split('\n').collect();
            if charset.len() < 2 || !charset[0].is_empty() {
                return Err("验证码字符集格式无效".into());
            }
            Ok(Self { session, charset })
        }

        pub fn recognize(&mut self, bytes: &[u8], mode: PreprocessMode) -> Result<String, String> {
            let (data, width) = preprocess(bytes, mode)?;
            let input = Tensor::from_array(([1usize, 1, 64, width], data))
                .map_err(|error| format!("tensor 构建失败: {error}"))?;
            let outputs = self
                .session
                .run(ort::inputs!["input1" => input])
                .map_err(|error| format!("模型推理失败: {error}"))?;
            let (shape, out) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|error| format!("输出解析失败: {error}"))?;
            let dims: Vec<usize> = shape.iter().map(|dim| (*dim).max(0) as usize).collect();
            if dims.len() != 3 || dims[1] != 1 {
                return Err(format!("输出形状异常: {dims:?}"));
            }
            let (steps, classes) = (dims[0], dims[2]);
            let mut text = String::new();
            let mut previous: i64 = -1;
            for step in 0..steps {
                let base = step * classes;
                let mut best = 0_usize;
                let mut best_value = f32::NEG_INFINITY;
                for class in 0..classes {
                    let value = out[base + class];
                    if value > best_value {
                        best_value = value;
                        best = class;
                    }
                }
                let index = best as i64;
                if index != previous && index != 0 {
                    if let Some(character) = self.charset.get(best) {
                        text.push_str(character);
                    }
                }
                previous = index;
            }
            Ok(text)
        }
    }

    pub fn available() -> bool {
        static ENGINE: OnceLock<Result<Mutex<OcrEngine>, String>> = OnceLock::new();
        ENGINE
            .get_or_init(|| OcrEngine::load().map(Mutex::new))
            .is_ok()
    }

    pub fn recognize_with(bytes: &[u8], mode: PreprocessMode) -> Result<String, String> {
        static ENGINE: OnceLock<Result<Mutex<OcrEngine>, String>> = OnceLock::new();
        let engine = ENGINE.get_or_init(|| OcrEngine::load().map(Mutex::new));
        match engine {
            Ok(engine) => {
                let mut guard = engine.lock().map_err(|_| "OCR 引擎锁失效".to_string())?;
                guard.recognize(bytes, mode)
            }
            Err(error) => Err(error.clone()),
        }
    }

    fn preprocess(bytes: &[u8], mode: PreprocessMode) -> Result<(Vec<f32>, usize), String> {
        let image = image::load_from_memory(bytes)
            .map_err(|error| format!("验证码图片解码失败: {error}"))?;
        let image = match mode {
            PreprocessMode::Baseline => image,
            PreprocessMode::BoundingBoxCrop => crop_to_text(image),
        };
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return Err("验证码图片尺寸非法".into());
        }
        let target_height = 64_u32;
        let target_width =
            (((width as f64) * (target_height as f64 / height as f64)) as u32).max(1);
        let resized = image.resize_exact(
            target_width,
            target_height,
            image::imageops::FilterType::CatmullRom,
        );
        let rgb = to_rgb_on_white(&resized);
        let mut data = vec![0_f32; (target_width * target_height) as usize];
        for (index, pixel) in rgb.pixels().enumerate() {
            let luma = 0.299 * f32::from(pixel[0])
                + 0.587 * f32::from(pixel[1])
                + 0.114 * f32::from(pixel[2]);
            data[index] = luma / 255.0;
        }
        Ok((data, target_width as usize))
    }

    fn to_rgb_on_white(image: &DynamicImage) -> RgbImage {
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        let mut rgb = RgbImage::new(width, height);
        for (x, y, pixel) in rgba.enumerate_pixels() {
            let alpha = f32::from(pixel[3]) / 255.0;
            let blend = |channel: u8| -> u8 {
                ((f32::from(channel) * alpha) + 255.0 * (1.0 - alpha)).round() as u8
            };
            rgb.put_pixel(
                x,
                y,
                Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]),
            );
        }
        rgb
    }

    /// Crop to dark pixels, then add a 4px white border. Falls back to the
    /// original image when the mask is implausibly small.
    fn crop_to_text(image: DynamicImage) -> DynamicImage {
        let rgb = to_rgb_on_white(&image);
        let (width, height) = rgb.dimensions();
        let mut min_x = width;
        let mut min_y = height;
        let mut max_x = 0_u32;
        let mut max_y = 0_u32;
        for (x, y, pixel) in rgb.enumerate_pixels() {
            let luma = 0.299 * f32::from(pixel[0])
                + 0.587 * f32::from(pixel[1])
                + 0.114 * f32::from(pixel[2]);
            if luma < 200.0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        if max_x <= min_x + 3 || max_y <= min_y + 3 {
            return DynamicImage::ImageRgb8(rgb);
        }
        let padding = 4_u32;
        let left = min_x.saturating_sub(padding);
        let top = min_y.saturating_sub(padding);
        let right = (max_x + padding + 1).min(width);
        let bottom = (max_y + padding + 1).min(height);
        let cropped = image.crop_imm(left, top, right - left, bottom - top);
        let mut canvas = RgbImage::from_pixel(
            cropped.width() + padding * 2,
            cropped.height() + padding * 2,
            Rgb([255, 255, 255]),
        );
        image::imageops::overlay(
            &mut canvas,
            &to_rgb_on_white(&cropped),
            i64::from(padding),
            i64::from(padding),
        );
        DynamicImage::ImageRgb8(canvas)
    }
}

#[cfg(feature = "captcha-ocr")]
pub use engine::{available as ocr_available, recognize_with};

#[cfg(not(feature = "captcha-ocr"))]
pub fn ocr_available() -> bool {
    false
}

#[cfg(not(feature = "captcha-ocr"))]
pub fn recognize_with(_bytes: &[u8], _mode: PreprocessMode) -> Result<String, String> {
    Err("本地验证码识别未编译（captcha-ocr feature 关闭）".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_data_uri() {
        assert!(decode_data_uri("https://example.com/captcha.png").is_err());
    }

    #[test]
    #[ignore = "requires KEYDOCK_CAPTCHA_SAMPLES pointing at the reference images"]
    fn reference_sample_crop_fixes_missing_character() {
        let Ok(dir) = std::env::var("KEYDOCK_CAPTCHA_SAMPLES") else {
            return;
        };
        let path = std::path::PathBuf::from(dir).join("c06.png");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let output = recognize(&bytes, Some(4)).expect("ocr");
        assert_eq!(output.text.to_lowercase(), "sffy");
        assert_eq!(output.mode, PreprocessMode::BoundingBoxCrop);
    }

    #[test]
    fn decodes_png_data_uri() {
        let png = b"\x89PNG\r\n\x1a\n";
        let uri = format!("data:image/png;base64,{}", B64.encode(png));
        assert_eq!(decode_data_uri(&uri).unwrap(), png);
    }
}
