# Winux-Kate

## 项目概述

Winux-Kate 是一个 Windows 桌面环境替代器（Desktop Shell），用 Tauri (Rust + React) 构建。它替代 explorer.exe，提供多页面工作空间，每页嵌入不同的应用窗口（终端、IDE、IM、桌面、自定义应用）。

作者：Deaicup 工作室

## 构建命令

```bash
# 前端 + 后端一起构建（release）
npx tauri build --no-bundle

# 仅前端开发
npm run dev

# 仅 Rust 检查
cd src-tauri && cargo check

# 构建产物
# src-tauri/target/release/winux-kate.exe
# 部署到 build/winux-kate.exe
```

## 架构

### 前端 (src/)
- React 18 + TypeScript + Vite
- 状态管理：Zustand (store.ts)
- 终端：xterm.js + ConPTY (后端 pty.rs)
- 页面组件：
  - `DashboardPage` (第1页): 双终端面板
  - `IdePage` (第2页): VSCode 多实例
  - `ImPage` (第3页): 微信/QQ/企业微信
  - `DesktopPage` (第4页): 桌面快捷方式 + 收纳窗口
  - `CustomPageView` (第5页+): 用户自定义应用（如 Trae）

### 后端 (src-tauri/src/)
- `lib.rs`: Tauri 入口，注册命令、窗口事件、退出清理
- `window_manager.rs`: 核心窗口管理（attach/detach/move/show/hide/pin_overlays/apply_layout）
- `apps.rs`: 应用启动/管理逻辑（IDE/IM/custom 页面）
- `commands.rs`: Tauri 命令定义（前端调用的 API）
- `state.rs`: 全局状态（AppState：managed windows, slots, instances）
- `pty.rs`: ConPTY 终端会话管理
- `hotkey.rs`: 全局热键（Ctrl+Shift+Tab 切换页面/实例）
- `shell.rs`: explorer.exe 杀/恢复
- `config.rs`: 持久化配置（custom pages）
- `shortcuts.rs`: 桌面快捷方式扫描
- `system.rs`: 系统信息

### 窗口管理核心概念
- **ManagedWindow**: 被 Kate 管理的外部窗口，有 slot/page/kind
- **SlotKind**: WeChat/Qq/WeCom/DesktopApp/Custom/Terminal/Ide
- **apply_layout**: 根据当前页面显示/隐藏/定位所有管理窗口
- **pin_overlays**: watchdog 定时器（~800ms），保持窗口位置和层级
- **attach_window**: 将外部窗口嵌入（strip decorations / set parent / move to slot）
- **park_offscreen**: 将窗口移到屏外 (-32000) 保持存活

## 关键约束

### Trae (Electron) 窗口管理限制
Trae 有自我保护机制，**任何外部窗口操作**都会触发 `Lifecycle#kill()`：
- `SetWindowPos`（移动/缩放）-> kill
- `ShowWindow(SW_HIDE)` -> kill
- `ShowWindow(SW_MINIMIZE)` -> kill
- `SetWindowLongPtrW`（改样式）-> kill

唯一安全的操作：`SetForegroundWindow`（仅切换前台焦点）。

第5页（CustomPage）对 Trae 的管理策略：
- 只调 `SetForegroundWindow` 把 active 窗口提到前台
- 不隐藏/最小化/移动非 active 窗口
- Kate 主窗口设为 `HWND_BOTTOM` 让 Trae 可点击
- 必须检查 `IsWindow` 跳过已死的 hwnd（Trae kill 后 hwnd 失效）
- 非第5页时恢复 Kate 主窗口为 `HWND_TOP`

### DashboardPage 终端持久化
- DashboardPage 常驻挂载（不随页面切换卸载），用 CSS `display:none` 隐藏
- 终端进程（如 claude）在切换页面后保持存活

### Kate 主窗口最小化
- `apply_layout` 检查 `IsIconic(main_hwnd)`，最小化时隐藏所有嵌入窗口

## 代码约定

- Rust：使用 `parking_lot::Mutex`（不用 std Mutex）
- HWND 存储为 `usize`（`hwnd_to_usize` / `hwnd_from_usize`）
- 前端用 `invoke<T>("command_name", { args })` 调用后端
- CSS 变量定义在 `theme.css`，页面样式在 `pages.css`

## 当前开发状态

- 第1-4页基本可用
- 第5页（自定义应用/Trae）：受限于 Trae 自我保护机制，只能做前台切换
- 已知问题：Trae 多实例切换时非 active 窗口无法隐藏（会重叠）
