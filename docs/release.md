# 发版流程

## 仓库

- GitHub：`XSIJIE975/keydock`，必须 public（匿名 updater 请求需要读取 Release 资产）。
- 默认分支：`main`；日常开发：`dev`。
- 工作流：
  - `ci.yml`：push 到 dev/main、PR 到 main；前端类型检查/构建 + Windows/macOS Rust 测试。
  - `version.yml`：push 到 main；changesets 创建 “Version Packages” PR。
  - `release.yml`：Version Packages PR 合并到 main，或手动触发；创建 draft release → 矩阵构建 → 从 `.sig` 重建 `latest.json`。

## 发一次新版本

1. 在 `dev` 上完成改动，运行 `pnpm changeset` 选择版本级别并写说明。
2. 推送 `dev`，合并到 `main`。
3. `version.yml` 打开 “Version Packages” PR；合并它。
4. `release.yml` 自动创建 draft release 并构建：
   - Windows x64：NSIS；
   - macOS arm64：DMG。
5. 检查 draft release 资产：
   - `*_x64-setup.exe` 与 `.sig`
   - `*_aarch64.dmg` 与 updater 的 `.app.tar.gz` / `.sig`
   - `latest.json`
6. 手动测试后发布 draft release。

也可手动触发 `release.yml`，填写版本号（例如 `0.1.0`）强制构建。

## 必需的 Secrets

| Secret | 说明 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | minisign 私钥内容；用于生成 updater `.sig` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 仅当私钥有密码时需要单独加入；本项目密钥无密码，工作流默认不设置 |

本地生成过一次密钥对：

- 私钥：`F:\Rea_temp\KeyDock\keydock-updater.key`（不要提交，建议另行备份）
- 公钥：已写入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`

如果私钥丢失，已安装客户端将无法验证新版本；必须更换公钥并发布一次完整安装包。

## 本地构建（Windows）

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "F:\Rea_temp\KeyDock\keydock-updater.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
pnpm tauri build --bundles nsis
```

产物：

- `src-tauri/target/release/bundle/nsis/KeyDock_<version>_x64-setup.exe`
- 同目录 `.sig`
- `src-tauri/target/release/bundle/updater/` 下的 updater 资产

## macOS

- 首发不签名、不公证；用户首次打开需右键 → 打开。
- CI 使用 `macos-latest` + `--target aarch64-apple-darwin --bundles app,dmg`；`app` 用于生成 updater 的 `.app.tar.gz`，`dmg` 用于安装包。
- Touch ID 标记为实验性；未签名替换后钥匙串授权可能失效，必须捕获并回退主密码。
- 后续补签名：
  1. `tauri.conf.json` 增加 `bundle.macOS.signingIdentity`；
  2. CI 导入证书与 `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID`（公证）；
  3. `latest.json` 不需要改。

## 更新清单

`fill-updater-notes.mjs` 不信任各平台自行上传的 `latest.json`，而是扫描 Release 的 `.sig` 资产确定性重建：

- `*_x64-setup.exe.sig` → `windows-x86_64`、`windows-x86_64-nsis`
- `*_aarch64.app.tar.gz.sig` / `*_aarch64.dmg.tar.gz.sig` → `darwin-aarch64`、`darwin-aarch64-app`

`notes` 来自 `release-body.md`（changeset 生成的 CHANGELOG 片段），应用内直接渲染。
