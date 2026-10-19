# ADR 0004: 验证码 OCR 预处理与插值

- 状态：Accepted
- 日期：2026-10-20

## 背景

交接文档要求用 `ort 2.0.0-rc.10` + ddddocr `common_old.onnx`，并在 Phase 3 用真实样本确定插值算法、写入单测锁定。

当前可用样本为 8 张带彩色干扰线的斜体验证码。人工判读：UZMD / SFFY / JGTZ / 7LSW / DWGE / JWX2 / MNJF / NB8V。

## 实测

| 插值 | 基线 | 包围盒裁剪 | 基线 + 长度校验后裁剪 |
|---|---:|---:|---:|
| Triangle | 5/8 | 7/8 | 7/8 |
| CatmullRom | 6/8 | 6/8 | 7/8 |

CatmullRom 基线把样本 3 修正为 `nb8v`，与交接文档“基线 6/8”一致；样本 6 为 `sff`，长度校验触发裁剪后修正为 `sffy`，达到 7/8。样本 2 `7esw` 仍为单字符错误。

## 决定

- 默认缩放使用 `image::imageops::FilterType::CatmullRom`。
- `recognize(bytes, expected_length)` 的遍历：
  1. 基线识别；
  2. 若配置了 expectedLength 且长度不符 → 包围盒裁剪（4px 白边）重试；
  3. 仍不合格 → 调用方刷新验证码并重试，最多 `maxRetries`；
  4. 全部失败 → 保留浏览器、高亮验证码输入框、转人工。
- 字符集文件首行为空，映射到 CTC blank；解码时跳过 blank 与连续重复。
- 模型与字符集用 `include_bytes!` / `include_str!` 内嵌，`captcha-ocr` feature 控制。

## 限制

- 8 张样本只证明链路可用，不代表生产准确率。
- 接入新站点应先采集 50–100 张验证码做离线对拍；低于 80% 时优先按站点微调小模型。
- 滑块、点选、行为验证码不做 OCR，直接 manual。
- 取图必须用资源字节，禁止截图；canvas 污染时降级 manual。

## 复现

```powershell
cargo run -p keydock-browser --example captcha_eval -- "F:\Rea_temp\DevDock\captcha-samples"
```
