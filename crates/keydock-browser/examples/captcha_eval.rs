//! Offline OCR evaluation against a directory of captcha images.
//! Usage: `cargo run -p keydock-browser --example captcha_eval -- <dir>`

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"F:\Rea_temp\DevDock\captcha-samples"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("png" | "jpg" | "jpeg")
            )
        })
        .collect();
    files.sort();

    println!("dir: {}", dir.display());
    println!(
        "ocr available: {}",
        keydock_browser::captcha::ocr_available()
    );
    for file in files {
        let bytes = std::fs::read(&file).expect("read image");
        let started = Instant::now();
        let baseline = keydock_browser::captcha::recognize_with(
            &bytes,
            keydock_browser::captcha::PreprocessMode::Baseline,
        );
        let crop = keydock_browser::captcha::recognize_with(
            &bytes,
            keydock_browser::captcha::PreprocessMode::BoundingBoxCrop,
        );
        let with_expectation = keydock_browser::captcha::recognize(&bytes, Some(4));
        println!(
            "{}  ->  baseline={:?} crop={:?} expectation={:?}  {}ms",
            file.file_name().unwrap().to_string_lossy(),
            baseline.map_err(|error| error.to_string()),
            crop.map_err(|error| error.to_string()),
            with_expectation.map(|output| (output.text, output.mode)),
            started.elapsed().as_millis()
        );
    }
}
