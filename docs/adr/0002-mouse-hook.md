# ADR 0002: Mouse4 低级钩子与径向覆盖层

- 状态：Windows 部分 Accepted；macOS 部分 Pending（本机无法执行）
- 日期：2026-10-20
- 范围：Phase 0 鼠标 spike

## 背景

径向菜单要求（D18）：

- 默认 Mouse4 长按 250ms 唤起；短按必须保留原点击行为。
- 覆盖层透明、置顶、不抢焦点、不处理 DOM 鼠标事件。
- 命中判定与多显示器边界计算必须是可脱离 GUI 的纯函数。
- 左键、右键永久禁止绑定。
- macOS 需要辅助功能授权，未授权时功能会静默失效，必须有授权引导。

Phase 0 在 Windows 上实现了 `WH_MOUSE_LL` 钩子、异步点击补发、8 扇区覆盖层、双屏定位和几何单测。

## 环境

- Windows `10.0.26300 x64`
- 进程 DPI 感知：`SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` 成功
- 两块显示器，实测为混合 DPI：
  - 左侧：`-2880..0 × 0..1800`，工作区 `-2880..0 × 0..1704`，192 DPI（200%）
  - 主屏：`0..3840 × 0..2160`，工作区 `0..3840 × 0..2088`，144 DPI（150%）
- 光标在测试结束后恢复原位置

## 结果

自测命令：`cargo run -p keydock-mouse-spike`
报告：`F:\Rea_temp\KeyDock\phase0\mouse-hook-report.json`（临时目录，不提交）

| 检查 | 结果 | 证据 |
|---|---|---|
| `WH_MOUSE_LL` 安装 | PASS | 非空 HHOOK |
| Mouse4 down/up 捕获 | PASS | 原始计数各 1 |
| 短按补发原点击 | PASS | 按下 81ms；目标窗口收到且只收到 1 个 down + 1 个 up |
| 短按不进入菜单 | PASS | `menu_shown` 无增量；无覆盖层 |
| 长按进入菜单 | PASS | 450ms 后覆盖层可见，恰好 1 次 |
| 覆盖层不抢焦点 | PASS | 显示前后 `GetForegroundWindow` 不变 |
| 8 扇区角度选择 | PASS | 主屏向东选 0；负坐标副屏向南选 2 |
| 菜单模式下不穿透点击 | PASS | 目标窗口 down 计数无增量 |
| 松手隐藏覆盖层 | PASS | `IsWindowVisible=false` |
| 主屏右下边界定位 | PASS | 覆盖层矩形完全落在工作区内 |
| 副屏负坐标定位 | PASS | 副屏长按覆盖层 `-1620,672..-1260,1032`，完全落在 `-2880,0..0,1704` 工作区 |
| 几何纯函数单测 | PASS | 5/5：居中、右/下翻转、负坐标、最近显示器、8 扇区 + 中心取消 |
| 重复稳定性 | PASS | 修复目标窗口置前逻辑后连续 3 次全过 |

## 关键实现结论

### 1. 低级钩子的 wParam 语义

`WH_MOUSE_LL` 回调的 `wParam` 是鼠标消息号（例如 `0x20C = WM_XBUTTONUP`）。
XButton 编号在 `MSLLHOOKSTRUCT.mouseData` 的高字（实测 `mouseData=0x00010000` 表示 XBUTTON1）。
不能像普通窗口消息那样从 `wParam >> 16` 取 XButton。这是本轮最重要的踩坑。

### 2. 不要在钩子回调内 `SendInput`

第一版在 `WM_XBUTTONUP` 回调里直接补发点击，结果目标窗口收到 1 个 down + 2 个 up：
原始 up 与补发 up 都到达了目标。改为：

1. 钩子回调只记录状态、`PostMessage(WM_APP_REPLAY)`，并返回 1 吞掉原事件；
2. 主线程消息循环收到自定义消息后再 `SendInput` 补发。

修复后目标窗口严格收到一对事件。补发事件带自定义 `dwExtraInfo`，钩子在入口处识别并放行，避免递归。

### 3. 覆盖层窗口属性

覆盖层使用：

```
WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST |
WS_EX_NOACTIVATE | WS_EX_TRANSPARENT
```

并以 `SW_SHOWNOACTIVATE` + `SWP_NOACTIVATE` 显示；`WM_MOUSEACTIVATE` 返回 `MA_NOACTIVATE`。
颜色键 + 整体 alpha 实现圆形外观与透明边角。

实测显示覆盖层前后前台窗口不变，满足“不抢焦点”。
这验证了平台机制；产品代码会使用 Tauri 窗口，其 `focusable(false)` 在 Windows 上
经 `tauri-runtime-wry` → `tao` 最终落到 `WS_EX_NOACTIVATE`（源码核对：
`tao-0.37.1/src/platform_impl/windows/window_state.rs`）。

本轮还尝试了一个 cargo 直跑的 Tauri 覆盖层冒烟 example，但它在启动时返回
`STATUS_ENTRYPOINT_NOT_FOUND`（0xc0000139）；同一时间编译出的 Tauri 主壳
`target/debug/keydock.exe` 可以正常启动。该 example 已移除，避免留下不可运行的探针。
Phase 2 必须通过 `pnpm tauri dev` / 正式打包路径再做一次真实 Tauri 覆盖层冒烟，
确认透明、置顶、不抢焦点三者同时成立；在此之前只算“平台机制已验证”。

### 4. 多显示器与 DPI

- 统一使用物理像素坐标；进程必须 PerMonitorV2 感知，否则混合 DPI 下定位会漂移。
- 用 `EnumDisplayMonitors` + `GetMonitorInfoW.rcWork` 取工作区；
  `GetDpiForMonitor(MDT_EFFECTIVE_DPI)` 记录每屏 DPI。
- 纯函数 `place_overlay` 以光标所在工作区为准，贴边时反向偏移并 clamp；
  光标不在任何工作区时选择最近显示器。
- 本轮实际环境是 192 DPI + 144 DPI 的双屏，且左屏坐标为负，覆盖了关键边界。

### 5. 时序与体验

- 短按在松开时补发，因此 Mouse4 的点击延迟约等于按住时长；81ms 的测试按住对应 81ms 延迟。
- 长按阈值 250ms；到达阈值后覆盖层立即出现。
- 默认不绑定左键/右键，保持交接文档的硬约束。
- 安全软件或远程控制软件可能拦截低级钩子；Phase 5 必须保留全局快捷键作为等价入口。

## macOS：未完成，Phase 0 出口尚未双平台闭合

本机是 Windows，无法执行 macOS 部分。Phase 0 的 macOS 出口项仍为 **Pending**，不能标记为通过。

后续实现要点（需要在 arm64 Mac 上实测）：

1. 用 `CGEventTap` 监听 `kCGEventOtherMouseDown` / `kCGEventOtherMouseUp`，
   只处理 buttonNumber == 3（Mouse4）。
2. 短按补发用 `CGEventPost`，带自定义 user data 标记，避免递归。
3. 辅助功能授权：先 `AXIsProcessTrustedWithOptions` 引导，再轮询授权状态；
   未授权时径向菜单静默失效，但面板内必须显示明确状态和“打开系统设置”入口。
4. 覆盖层使用非激活的 NSPanel/NSWindow，`ignoresMouseEvents = true`，
   `collectionBehavior` 包含 `canJoinAllSpaces` / `fullScreenAuxiliary`，不成为 key window。
5. 多屏坐标使用全局屏幕坐标；Retina 下区分 points 与 backing pixels。
6. 长按期间不能阻塞 event tap 超时；补发必须异步。
7. 获取一台 macOS arm64 测试机后，重新走本 ADR 的短按、长按、焦点、双屏、混合缩放矩阵。

在完成 macOS 实测前，Phase 5 的径向菜单不能宣称双平台可用。

## 限制

- 本轮覆盖层是 Win32 spike 窗口，不是产品 UI；正式径向菜单仍按 §9.5 实现。
- 只验证了 Mouse4，未验证 Mouse5/中键；配置层会在 Phase 5 限制为 Mouse4/Mouse5/中键。
- 未测试安全软件拦截、远程桌面、锁屏界面、UAC 安全桌面。
- 自测会把测试目标窗口显式置前，以确保注入点击有稳定落点；覆盖层是否抢焦点仍是在此基础上独立检查的。
  产品中的径向菜单不需要也不应该主动激活任何窗口。
- 未进行任何截图或屏幕捕获；所有判断来自 Win32 状态、窗口消息计数和 DOM/日志。

## 复现

```powershell
cargo test -p keydock-mouse-spike
cargo run -p keydock-mouse-spike
```

测试会短暂显示一个测试窗口和覆盖层，自动注入 Mouse4，结束后恢复光标并退出。
