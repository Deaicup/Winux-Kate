# Winux-Kate 桌面系统 — 初始化开发计划

> 一款用 Tauri v2 + Rust + React/TS 构建的 Windows 桌面 Shell 替代系统，集成 eDEX-UI 风格界面、终端、文件管理器、文本编辑器、内置窗口管理器、VSCode IDE、微信/企微/QQ 固定窗口，分 4 页，用 Ctrl+Tab / Ctrl+Shift+Tab 切换。

---

## 1. 摘要

本次「初始化开发」交付：**完整骨架 + 核心可跑**。即：
- 项目脚手架（Tauri v2 + React + TS + Vite）。
- 替换 explorer.exe 作为系统 Shell 的能力（注册表写入 + 启动时杀掉 explorer，开发态安全开关）。
- 全屏无边框主窗口作为桌面背景。
- 4 页分页框架 + 全局低层键盘钩子（Ctrl+Tab 切页 / Ctrl+Shift+Tab 上下文相关操作）。
- 内置窗口管理器核心：枚举/启动外部进程，用 `SetParent` 把外部窗口 reparent 到主窗口并按槽位定位、随页显隐。
- 第 1 页可用：双终端（ConPTY + xterm.js）、Monaco 文本编辑器、文件查看器、状态栏（时间/音量/亮度/蓝牙/WiFi）。
- 第 2 页：启动并全屏嵌入 VSCode，多实例管理（Ctrl+Shift+Tab 切换/新建）。
- 第 3 页：微信 + QQ 左右分屏嵌入，Ctrl+Shift+Tab 切换企微。
- 第 4 页：读取 Windows 桌面 `.lnk` 快捷方式并以图标展示，点击启动进程并将其窗口 reparent 为第 4 页子窗口。
- eDEX-UI 科幻主题（深色背景、霓虹青/绿、等宽字体、发光边框、启动序列）。

---

## 2. 现状分析

仓库 `d:\Code\Winux-Kate` 当前为空项目，仅含：
- [README.md](file:///d:/Code/Winux-Kate/README.md)（仅一行标题 `# Winux-Kate`）
- [LICENSE](file:///d:/Code/Winux-Kate/LICENSE)（MIT，Copyright 2026 Deaicup）

无任何源码、构建配置或依赖。属于全新 greenfield 项目，可自由选择技术结构与目录布局。

---

## 3. 技术决策

| 决策项 | 选择 | 理由 |
|---|---|---|
| 应用框架 | **Tauri v2**（稳定版，2024-10 发布） | 轻量二进制，Rust 后端直接调用 Win32，前端用 Web 实现 eDEX-UI 风格 |
| 后端语言 | **Rust** | 通过官方 `windows` crate 获得最完整 Win32/COM/WinRT 访问 |
| 前端 | **React 18 + TypeScript + Vite** | 组件复用、生态成熟；xterm.js / Monaco 均有 React 封装 |
| 终端后端 | **ConPTY**（经 `portable-pty` crate 封装） | Windows 原生伪终端，兼容 powershell/cmd |
| 终端前端 | **@xterm/xterm + @xterm/addon-fit** | 行业标准 Web 终端 |
| 编辑器 | **@monaco-editor/react** | VSCode 同款编辑器内核 |
| Win32 绑定 | **`windows` crate**（Microsoft 官方） | SetParent/EnumWindows/Core Audio/WMI/COM/WinRT 一站式 |
| 全局热键 | **WH_KEYBOARD_LL 低层键盘钩子**（自建线程+消息循环） | 外部嵌入窗口抢焦点时仍能捕获 Ctrl+Tab，shell 级窗口管理必需 |
| Shell 替换 | 写 `HKLM\...\Winlogon\Shell` 注册表 + 启动杀 explorer | 文档验证的标准方案；开发期用环境开关避免误操作 |

### 关键 Rust 依赖（`src-tauri/Cargo.toml`）
- `tauri = { version = "2", features = ["..."] }`
- `windows = { version = "0.x", features = ["Win32_Foundation","Win32_UI_WindowsAndMessaging","Win32_System_Threading","Win32_System_LibraryLoader","Win32_Graphics_Gdi","Win32_Media_Audio","Win32_System_Com","Win32_UI_Shell","Win32_UI_Shell_Common","Win32_System_Registry","Win32_Devices_Bluetooth","Win32_NetworkManagement_Wlan","Win32_System_Console"] }`
- `portable-pty = "0.8"`（ConPTY 封装）
- `wmi = "0.14"`（亮度等 WMI 查询）
- `serde`, `serde_json`, `parking_lot`（共享状态锁）, `once_cell`
- `log` + `env_logger`

### 关键前端依赖（`package.json`）
- `@tauri-apps/api` v2、`@tauri-apps/plugin-shell` v2
- `react`、`react-dom`、`@xterm/xterm`、`@xterm/addon-fit`、`@monaco-editor/react`
- `vite`、`@vitejs/plugin-react`、`typescript`

---

## 4. 架构总览

```
┌─────────────────────────────────────────────────────────────┐
│  Winux-Kate.exe（Tauri 主进程 = Windows Shell）              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │ Rust 后端（src-tauri/src）                             │  │
│  │  shell.rs        注册表 Shell 替换 / 杀 explorer       │  │
│  │  window_manager.rs 枚举/SetParent/定位/显隐 外部窗口   │  │
│  │  hotkey.rs       WH_KEYBOARD_LL 钩子 → 页面切换事件    │  │
│  │  pty.rs          ConPTY 双终端会话                     │  │
│  │  system.rs       音量/亮度/蓝牙/WiFi/时间              │  │
│  │  shortcuts.rs    枚举+解析桌面 .lnk                    │  │
│  │  apps.rs         启动/追踪 VSCode/微信/QQ/企微         │  │
│  │  commands.rs     暴露给前端的 #[tauri::command]        │  │
│  │  state.rs        全局共享状态（当前页/槽位/HWND 表）    │  │
│  └───────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────┐  │
│  │ WebView2 前端（src，React）—— 渲染 4 页 UI 框架         │  │
│  │  Page1 Dashboard │ Page2 IDE │ Page3 IM │ Page4 Desktop│  │
│  └───────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────┐  │
│  │ 被 reparent 的外部原生窗口（覆盖在 WebView 指定槽位）   │  │
│  │  VSCode │ 微信 │ QQ │ 企微 │ 第4页启动的任意进程窗口   │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

**核心运行模型：**
1. 主 Tauri 窗口全屏无边框，覆盖整个屏幕，承载 WebView（渲染页面 UI 框架）。
2. 外部进程窗口经 `SetParent` 挂为主窗口子窗口，由 Rust 按前端给的槽位像素矩形 `SetWindowPos` 定位，叠在 WebView 之上。
3. 切页时：Rust 隐藏（`SW_HIDE`）非当前页的子窗口，显示并重定位当前页的子窗口；前端同步切换可见的 React 页面。
4. 全局键盘钩子拦截 Ctrl+Tab / Ctrl+Shift+Tab，更新共享状态中的「当前页」，发 Tauri 事件通知前端，并触发窗口显隐。

---

## 5. 目录结构

```
Winux-Kate/
├── package.json
├── vite.config.ts
├── tsconfig.json
├── index.html
├── src/                              # 前端
│   ├── main.tsx
│   ├── App.tsx                       # 根组件 + 页面路由 + Tauri 事件监听
│   ├── store.ts                      # zustand 全局状态（当前页/槽位/实例列表）
│   ├── pages/
│   │   ├── DashboardPage.tsx         # 第1页
│   │   ├── IdePage.tsx               # 第2页
│   │   ├── ImPage.tsx                # 第3页
│   │   └── DesktopPage.tsx           # 第4页
│   ├── components/
│   │   ├── Terminal.tsx              # xterm.js 终端
│   │   ├── CodeEditor.tsx            # Monaco 编辑器
│   │   ├── FileViewer.tsx            # 文件查看器
│   │   ├── StatusBar.tsx             # 时间/音量/亮度/蓝牙/WiFi
│   │   ├── PageSwitcher.tsx          # 页面指示器
│   │   └── DesktopGrid.tsx           # 第4页快捷方式网格
│   ├── hooks/
│   │   ├── useTauriEvents.ts
│   │   └── useLayoutRects.ts         # 计算并向 Rust 上报槽位像素矩形
│   └── styles/
│       ├── theme.css                 # eDEX-UI 变量（颜色/字体/发光）
│       ├── boot.css                  # 启动序列动画
│       └── global.css
└── src-tauri/
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json
    ├── capabilities/default.json
    ├── icons/
    └── src/
        ├── main.rs                   # 入口（windows 子系统，无控制台）
        ├── lib.rs                    # tauri::Builder 装配插件/命令/状态
        ├── shell.rs                  # Shell 替换 + 杀 explorer
        ├── state.rs                  # AppState（当前页、HWND 表、槽位表）
        ├── window_manager.rs         # 外部窗口枚举/reparent/定位/显隐
        ├── hotkey.rs                 # 低层键盘钩子
        ├── pty.rs                    # ConPTY 会话管理
        ├── system.rs                 # 音量/亮度/蓝牙/WiFi/时间
        ├── shortcuts.rs              # 桌面 .lnk 解析
        ├── apps.rs                   # 启动 VSCode/微信/QQ/企微
        └── commands.rs               # #[tauri::command] 汇总
```

---

## 6. 提议的变更（分阶段实现）

### 阶段 A — 项目脚手架
**文件：** 全新创建 `package.json`、`vite.config.ts`、`tsconfig.json`、`index.html`、`src/main.tsx`、`src/App.tsx`、`src-tauri/Cargo.toml`、`src-tauri/build.rs`、`src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json`、`src-tauri/src/main.rs`、`src-tauri/src/lib.rs`

- 用 `npm create tauri-app@latest` 等价的手工结构初始化 Tauri v2 + React + TS + Vite 工程。
- `tauri.conf.json` 关键配置：
  - 主窗口 `label: "main"`，`fullscreen: false` / `maximized: true`，`decorations: false`，`resizable: true`，`alwaysOnTop: false`，`title: "Winux-Kate"`。
  - `app.withGlobalTauri: true`，前端 `devUrl` 指向 Vite，`frontendDist` 指向 `../dist`。
  - `identifier: "com.deaicup.winux-kate"`。
- `main.rs`：`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`，调用 `winux_kate_lib::run()`。
- `lib.rs`：`tauri::Builder::default().plugin(tauri_plugin_shell::init()).manage(state).invoke_handler(tauri::generate_handler![...]).setup(...).run(...)`。
- 验证：`npm install` + `npm run tauri dev` 能弹出空白全屏无边框窗口。

### 阶段 B — Shell 替换与 explorer 管理
**文件：** `src-tauri/src/shell.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`

- `shell.rs`：
  - `install_as_shell(exe_path: &str)`：写 `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell`（需管理员）。提供 `uninstall_shell()` 恢复 `explorer.exe`。
  - `kill_explorer()`：枚举进程 `explorer.exe` 并 `TerminateProcess`。仅在环境变量 `WINUX_SHELL_MODE=1` 或运行参数 `--shell` 时执行，开发期默认不杀。
  - `is_running_as_shell()`：判断是否由 Winlogon 启动（检查父进程）。
- 暴露命令：`install_shell`、`uninstall_shell`、`kill_explorer`。
- **安全**：提供 `restore_explorer` 命令与退出时自动重启 explorer 的开发兜底。
- 验证：以管理员运行 `install_shell` 后注销重登，Winux-Kate 作为桌面启动；`uninstall_shell` 可还原。

### 阶段 C — 窗口管理器核心
**文件：** `src-tauri/src/window_manager.rs`、`src-tauri/src/state.rs`、`src-tauri/src/commands.rs`

- `state.rs`：`AppState` 含 `current_page: u8`、`slots: HashMap<SlotId, Slot>`、`managed: HashMap<SlotId, HWND>`，用 `ParkingLotMutex` 保护。
  - `Slot { page: u8, rect: Rect{x,y,w,h}, kind: SlotKind }`，`SlotKind ∈ {Terminal, Ide, WeChat, QQ, WeCom, DesktopApp}`。
- `window_manager.rs`：
  - `find_window_by_process(name: &str) -> Option<HWND>`：`EnumWindows` + `GetWindowThreadProcessId` + 进程名比对。
  - `attach(hwnd: HWND, parent: HWND)`：`SetParent`；`SetWindowLongPtrW(GWL_STYLE)` 去掉 `WS_POPUP|WS_CAPTION|WS_THICKFRAME|WS_SYSMENU|WS_MINIMIZEBOX|WS_MAXIMIZEBOX`，加 `WS_CHILD`；`SetWindowPos` 到槽位；`ShowWindow(SW_SHOW)`。
  - `detach(hwnd)`：恢复样式、解除父子、`ShowWindow(SW_HIDE)`。
  - `apply_layout(state)`：遍历 `managed`，当前页的窗口 `SW_SHOW`+`SetWindowPos` 到其槽位 rect，其余 `SW_HIDE`。
  - `launch_and_attach(exe, args, slot)`：`CreateProcessW` 启动，轮询等待主窗口出现（最多 N 秒），再 `attach`。
- 主窗口 HWND 获取：`app.get_webview_window("main").unwrap().hwnd()`。
- 监听主窗口 `WindowEvent::Resized` → 通知前端重算 rect → 前端上报 → `apply_layout`。
- 暴露命令：`report_slot_rects(rects: Vec<SlotRect>)`、`hide_all_external()`、`get_current_page()`。
- 验证：手动调用 `launch_and_attach("notepad.exe", ..., slot=DesktopApp)` 能把记事本嵌入主窗口指定区域并随页显隐。

### 阶段 D — 分页 + 全局热键
**文件：** `src-tauri/src/hotkey.rs`、`src-tauri/src/state.rs`、`src-tauri/src/lib.rs`、`src/App.tsx`、`src/store.ts`、`src/hooks/useTauriEvents.ts`

- `hotkey.rs`：
  - 专用线程 + `SetWindowsHookExW(WH_KEYBOARD_LL, callback, ..)` + 自有消息循环（`GetMessage`）。
  - 回调检测 `VK_TAB` + Ctrl 按下：
    - **Ctrl+Tab**：`current_page = current_page % 4 + 1`，发事件 `page-changed`，调 `apply_layout`，吞掉按键。
    - **Ctrl+Shift+Tab**（按当前页分支）：
      - 页 1 / 页 4：`current_page = (current_page + 2) % 4 + 1`（上一页）。
      - 页 2：触发 `ide-cycle` 事件（前端/Rust 循环 VSCode 实例；末尾再按则发 `ide-new` 打开文件夹）。
      - 页 3：触发 `im-toggle` 事件（微信/QQ ↔ 企微）。
  - 通过 `AppHandle::emit` 推送事件给前端。
- `lib.rs` setup 中 `std::thread::spawn(hotkey::start)`，持有钩子句柄，退出时 `UnhookWindowsHookEx`。
- 前端 `useTauriEvents` 监听 `page-changed`/`ide-cycle`/`im-toggle` 更新 `store`。
- `store.ts`（zustand）：`currentPage`、`ideList`、`imView` 等。
- `App.tsx`：根据 `currentPage` 渲染对应 Page 组件，外加 `PageSwitcher` 指示器。
- 验证：Ctrl+Tab 在 4 页间循环；焦点在嵌入窗口内时仍能切页；Ctrl+Shift+Tab 上下文行为正确。

### 阶段 E — 第 1 页 Dashboard
**文件：** `src-tauri/src/pty.rs`、`src-tauri/src/system.rs`、`src-tauri/src/commands.rs`、`src/pages/DashboardPage.tsx`、`src/components/Terminal.tsx`、`CodeEditor.tsx`、`FileViewer.tsx`、`StatusBar.tsx`、`src/hooks/useLayoutRects.ts`

- **双终端**：
  - `pty.rs`：`PtySession` 持有 `portable_pty` 的 `PtyPair` + 子进程（默认 `powershell.exe`，可配置 `cmd`）。`spawn(cmd)` 启动；读线程把 stdout 经 `AppHandle::emit("pty-data", {id, data})` 推前端；`write(id, bytes)` 命令写 stdin；`resize(id, cols, rows)`。
  - `Terminal.tsx`：xterm.js 实例，`fit` 自适应；监听 `pty-data` 写入；用户输入 `onData` → `invoke('pty_write', {id, data})`；`onResize` → `invoke('pty_resize',...)`。
  - 槽位：终端 1 / 终端 2 各占 Dashboard 左侧两格。
- **文本编辑器**：`CodeEditor.tsx` 用 `@monaco-editor/react`，纯前端打开/保存本地文件经 `tauri-plugin-fs` 或自定义命令（`read_file`/`write_file`）。
- **文件查看器**：`FileViewer.tsx` 列目录（`list_dir` 命令，返回 `Vec<Entry>`），点击文本文件在编辑器打开、二进制显示信息。
- **状态栏**：`StatusBar.tsx` 定时 `invoke('system_status')` 拉取 `{time, volume, muted, brightness, bluetooth_on, wifi_ssid, wifi_connected}`：
  - 时间：`chrono::Local::now()`。
  - 音量：Core Audio `IMMDeviceEnumerator` → `IAudioEndpointVolume::GetMasterVolumeLevelScalar` / `SetMute`。
  - 亮度：WMI `WmiMonitorBrightnessMethods` 的 `WmiSetBrightness`；读 `WmiMonitorBrightness`。
  - 蓝牙：`windows` crate `BluetoothFindFirstRadio`/`BluetoothGetRadioInfo` 判断可用与状态。
  - WiFi：`WlanOpenHandle` + `WlanEnumInterfaces` + `WlanQueryInterface`(current_connection) 取 SSID；或 WinRT `Windows.Devices.WiFi`。
  - 前端提供音量滑块、亮度滑块、蓝牙/WiFi 状态显示（点击可扩展为面板，本期只显示+音量/亮度可调）。
- `useLayoutRects`：用 `ResizeObserver` 测量各槽位 DOM 的屏幕像素矩形，`invoke('report_slot_rects', {rects})` 上报（第1页无外部嵌入窗口，但机制统一）。
- 验证：两个终端可输入命令并见输出；编辑器能改文件；状态栏实时刷新且音量/亮度可调。

### 阶段 F — 第 2 页 IDE（VSCode 嵌入）
**文件：** `src-tauri/src/apps.rs`、`src-tauri/src/commands.rs`、`src/pages/IdePage.tsx`

- `apps.rs`：
  - `launch_vscode(folder: Option<PathBuf>) -> HWND`：定位 `code.exe`（查 `%LOCALAPPDATA%\Programs\Microsoft VS Code\Code.exe` 及 PATH），`CreateProcessW` 启动（带 `--folder-uri` 或路径参数），轮询拿主窗口 HWND。
  - 维护 `Vec<VscodeInstance { hwnd, folder, title }>`。
- 行为：
  - 进入第 2 页时若实例列表为空，自动启动一个 VSCode（无文件夹）。
  - 当前活动 VSCode 全屏嵌入第 2 页整页槽位。
  - `ide-cycle`：活动指针后移；到末尾再触发 → 弹原生文件夹选择（`IFileOpenDialog`）→ 启动新实例并设为活动 → reparent。
  - `ide-new`：直接打开文件夹选择 → 新实例。
  - 非活动实例 `SW_HIDE`。
- `IdePage.tsx`：顶部细条显示实例 tab 列表（文件夹名）+「+ 新建」按钮，下方为嵌入区（占满，rect 上报）；Ctrl+Shift+Tab 切换由热键驱动，UI 同步高亮。
- 暴露命令：`ide_new`、`ide_cycle`、`ide_list`、`ide_close(index)`。
- 验证：VSCode 全屏可操作；Ctrl+Shift+Tab 在多实例间切换并能在末尾新建（打开文件夹）。

### 阶段 G — 第 3 页 IM（微信/QQ/企微）
**文件：** `src-tauri/src/apps.rs`、`src/pages/ImPage.tsx`

- `apps.rs`：
  - `launch_im(app: ImApp) -> HWND`：`ImApp ∈ {WeChat, QQ, WeCom}`。先 `find_window_by_process` 找已运行实例的窗口；找不到则按安装路径启动（路径可通过配置文件 `im_paths.json` 或注册表常见位置查找，本期支持手动配置路径）。
  - 微信进程名 `WeChat.exe`、QQ `QQ.exe`、企微 `WXWork.exe`。
- 布局：
  - 默认视图：微信左半屏、QQ 右半屏（各 50%）。
  - 企微视图：企微占满整页。
  - `im-toggle` 事件在两视图间切换；切到企微时隐藏微信/QQ，反之隐藏企微。
- `ImPage.tsx`：渲染左右两个槽位占位 DOM（用于上报 rect），顶部小条显示当前视图与切换提示。
- 暴露命令：`im_launch`、`im_toggle`、`im_set_paths`。
- 验证：微信/QQ 分屏显示并可操作；Ctrl+Shift+Tab 切到企微满屏，再切回分屏。

### 阶段 H — 第 4 页 Desktop（快捷方式启动）
**文件：** `src-tauri/src/shortcuts.rs`、`src-tauri/src/commands.rs`、`src/pages/DesktopPage.tsx`、`src/components/DesktopGrid.tsx`

- `shortcuts.rs`：
  - `list_desktop_shortcuts() -> Vec<Shortcut>`：扫描 `%USERPROFILE%\Desktop` 与 `C:\Users\Public\Desktop` 下的 `.lnk`（及 `.url`）。
  - `resolve_lnk(path) -> (target_path, args, icon, name)`：COM 初始化（`CoInitializeEx`）→ `IShellLinkW` + `IPersistFile::Load` → `GetPath`/`GetArguments`/`GetIconLocation`；图标用 `SHGetFileInfo` 取系统图标 HICON，转 RGBA 经 base64 给前端（或缓存为临时 png）。
- `DesktopPage.tsx` / `DesktopGrid.tsx`：网格展示图标+名称，双击/单击启动。
- 启动：`launch_desktop_app(target, args)` → `CreateProcessW` → 等主窗口 → `attach` 到第 4 页一个动态槽位（按可用区域平铺或浮动）。
  - 第 4 页嵌入窗口都标 `kind=DesktopApp`，`page=4`；切离第 4 页时全部 `SW_HIDE`。
  - 支持关闭：右键/按钮 `detach`+`TerminateProcess` 或仅解除嵌入让其独立。
- 暴露命令：`list_shortcuts`、`launch_app`、`close_app(slot)`。
- 验证：桌面快捷方式正确列出（含图标）；点击启动记事本/浏览器等并被嵌入第 4 页，不会跑到其他页。

### 阶段 I — eDEX-UI 主题与启动序列
**文件：** `src/styles/theme.css`、`boot.css`、`global.css`、`src/App.tsx`（启动序列分支）

- `theme.css`：CSS 变量——`--bg:#02040a`、`--panel:#0a1228cc`、`--accent:#00e5ff`（青）、`--accent2:#39ff14`（绿）、`--text:#cfeffb`；等宽字体 `'JetBrains Mono','Fira Code',monospace`；发光边框 `box-shadow:0 0 8px var(--accent)`；扫描线/网格背景。
- 面板样式：切角边框、半透明、左上角小标签（仿 eDEX-UI 面板头）。
- `boot.css`：启动时全屏黑色 + 逐行打字日志（"INITIALIZING WINUX-KATE SHELL..."）+ 进度条，~2.5s 后淡出进入桌面。
- 全局滚动条、选中色、按钮霓虹悬停态。
- 验证：视觉接近 eDEX-UI；启动序列顺滑。

---

## 7. 关键技术要点（Win32 细节）

1. **reparent 嵌入**：`SetParent(child, main)` 后必须用 `SetWindowLongPtrW(GWL_STYLE, (style & !WS_POPUP & !WS_CAPTION & !WS_THICKFRAME & !WS_SYSMENU) | WS_CHILD)`，并 `SetWindowPos(child, HWND_TOP, x, y, w, h, SWP_FRAMECHANGED|SWP_SHOWWINDOW)`。部分 Electron 应用（VSCode/微信）reparent 后菜单/输入仍可用；若异常则回退方案：不 reparent，改为 `SetWindowPos(HWND_TOPMOST)` 精确贴齐槽位 + 按页显隐（已在 `window_manager.rs` 预留 `attach_mode: Reparent|Overlay`）。
2. **主窗口 HWND**：`WebviewWindow::hwnd()` 返回 `Result<HWND>`；reparent 父目标即此 HWND。WebView2 本身是主窗口的子 HWND，被 reparent 的外部窗口同为子 HWND，按 z-order 叠在 WebView 之上即可见。
3. **键盘钩子线程**：`WH_KEYBOARD_LL` 必须在有消息循环的线程注册；用独立线程 `GetMessageW` 循环。回调里避免阻塞，仅做状态更新与 `emit`，重活交主线程 `run_on_main_thread`。
4. **Shell 注册表**：`HKLM\...\Winlogon\Shell` 改为 Winux-Kate.exe 绝对路径；需管理员。开发期可用 `--shell` 参数才执行 `kill_explorer`，避免误锁系统。提供 `restore_explorer` 兜底命令。
5. **ConPTY**：`portable_pty` 屏蔽差异；读循环把 bytes 经 Tauri 事件推前端，注意背压（前端渲染跟不上时丢弃旧帧或合批）。
6. **COM 线程模型**：`shortcuts.rs` 的 .lnk 解析在调用线程 `CoInitializeEx(COINIT_APARTMENTTHREADED)`，用完 `CoUninitialize`；或集中在专用 STA 线程。
7. **槽位坐标**：前端用 `getBoundingClientRect()` + `window.devicePixelRatio` 换算到物理像素上报；DPI 变化时重算。

---

## 8. 假设与决策

1. **前端框架**选用 React + TS + Vite（未问，作为合理默认；生态最适合 xterm/Monaco）。
2. **Ctrl+Shift+Tab 上下文行为**：
   - 页 1 / 页 4 → 上一页；
   - 页 2 → 循环 VSCode 实例，末尾再触发则新建（打开文件夹）；
   - 页 3 → 微信/QQ 分屏 ↔ 企微满屏 切换。
   - （若需统一为「上一页」，可在阶段 D 调整，但会与页2/3的子导航冲突，故采用上下文相关。）
3. **第 1 页终端**默认启动 `powershell.exe`，可在配置中改 `cmd.exe` 或其他。
4. **IM 应用路径**：微信/QQ/企微安装路径多变，首期支持手动配置（`im_paths.json` 或设置面板），并优先抓取已运行实例窗口；自动探测留待后续。
5. **图标渲染**：`.lnk` 图标通过 `SHGetFileInfo` 取系统图标位图，编码为 base64 data-url 传前端（不落地文件）。
6. **开发安全**：默认**不**写注册表、**不**杀 explorer；仅 `--shell` 模式或显式 `install_shell` 命令才生效，退出 dev 时自动恢复 explorer，防止开发机变砖。
7. **嵌入回退**：若某应用 reparent 异常，`window_manager` 支持切到 Overlay 模式（贴齐+置顶+显隐），保证可用。
8. **范围边界**：本次不做——多显示器扩展、UAC 提权弹窗接管、系统托盘整合、主题切换器、设置 GUI（路径配置除外）、自更新安装器。这些列入后续迭代。

---

## 9. 验证步骤

1. `npm install` 成功；`npm run tauri dev` 启动全屏无边框窗口，无控制台报错。
2. 阶段 B：管理员下 `invoke('install_shell')` 写入注册表正确；`invoke('kill_explorer', {mode:'shell'})` 后任务栏消失；`invoke('restore_explorer')` 可恢复。
3. 阶段 C：`invoke('launch_and_attach', {exe:'notepad.exe', slot:'desktop-1'})` 后记事本出现在主窗口指定矩形内；切页后隐藏。
4. 阶段 D：焦点在记事本内时按 Ctrl+Tab 仍能切页；Ctrl+Shift+Tab 在页2/3触发对应事件。
5. 阶段 E：两个终端独立可交互；`ipconfig` 等命令输出正常；状态栏每秒刷新，音量/亮度滑块即时生效。
6. 阶段 F：VSCode 全屏可编辑；打开多文件夹后 Ctrl+Shift+Tab 循环并在末尾弹出文件夹选择新建。
7. 阶段 G：微信/QQ 分屏可操作；Ctrl+Shift+Tab 切企微满屏，再切回。
8. 阶段 H：桌面快捷方式图标与名称正确；点击启动的应用嵌入第 4 页，切到其他页该窗口隐藏不外溢。
9. 阶段 I：启动序列展示后进入桌面；整体视觉呈 eDEX-UI 霓虹科幻风。
10. 以 Shell 身份注销重登：Winux-Kate 自动作为桌面启动，无 explorer 任务栏。

---

## 10. 后续迭代（不在本期）

- 多显示器 / 虚拟桌面扩展。
- UAC 提权弹窗的接管与安全策略。
- 系统托盘、通知中心、剪贴板历史的 eDEX 风格化。
- 主题/布局可视化设置 GUI。
- 安装器与自动更新（含还原 explorer 的安全网）。
- 第 4 页窗口的平铺/层叠/拖拽调整窗口管理增强。
