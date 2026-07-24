# Lessons Learned - Winux-Kate

## 2026-07-22 - Trae 窗口管理与多实例切换

### 经验

1. **SetForegroundWindow 是唯一安全的 Trae 窗口操作**: Trae (Electron) 对任何外部窗口操作（SetWindowPos、ShowWindow HIDE/MINIMIZE、SetWindowLongPtrW）都触发 `Lifecycle#kill()`。只有 `SetForegroundWindow` 不会触发 kill。
2. **HWND_BOTTOM 技巧**: 第5页时把 Kate 主窗口设为 `HWND_BOTTOM`（纯 z-order，SWP_NOMOVE|SWP_NOSIZE），让 Trae 窗口在上面可点击。切到其它页时恢复 `HWND_TOP`。
3. **IsWindow 检查必须做**: Trae kill 后 hwnd 失效，但实例列表还存着。pin_overlays 必须先检查 `IsWindow(hh)` 跳过死窗口，否则会继续把 Kate 设为 BOTTOM 导致所有窗口冒出。
4. **终端持久化用 display:none**: DashboardPage 常驻挂载，切换页面用 CSS `display:none` 隐藏而非卸载组件，终端进程（claude 等）保持存活。
5. **collect_all_top_level_windows**: Electron 应用新建的窗口可能初始为隐藏状态，`collect_visible_windows`（只返回 IsWindowVisible）找不到。需要枚举所有顶层窗口再按标题过滤。
6. **exclude 列表要从 custom_instances 收集**: managed map 用单一 key `custom-{page_id}` 只存一个窗口，不能用作 exclude 来源。必须从 `custom_instances` 的 Vec 收集所有已 adopt 的 hwnd。

### 教训

1. **不要对 Trae 窗口做 ShowWindow 操作**: 即使是 SW_HIDE（隐藏）也会触发 kill。SW_MINIMIZE 一样会 kill。曾经尝试 SW_HIDE -> Trae kill，SW_MINIMIZE -> Trae kill，最终只能完全不操作非 active 窗口。
2. **auto-adopt 新窗口导致无限增长**: 之前 discover_custom_windows 自动 adopt Trae 新建的窗口，导致 Trae kill 重启 -> 新窗口 -> adopt -> Trae 又 kill -> 恶性循环，instances 从 2 涨到 8。改为手动「接管新窗口」按钮。
3. **MSCTFIME UI 等系统窗口需要过滤**: adoptable_windows_filtered 必须过滤掉系统 IME 窗口（MSCTFIME、Default IME），否则它们会被收纳到桌面页。
4. **Kate 最小化时 pin_overlays 仍在运行**: 如果不检查 IsIconic(main_hwnd)，最小化后桌面窗口仍被 show_topmost，覆盖整个屏幕看起来像死机。
5. **条件渲染 vs 常驻挂载**: 页面用 `{currentPage === 1 && <DashboardPage />}` 条件渲染会导致切换页面时 Terminal 组件卸载 -> pty_kill -> 进程被杀。改为常驻挂载 + display:none。
6. **find_all_windows_by_processes 需要包含隐藏窗口**: 只用 collect_visible_windows 会漏掉 Electron 新建的初始隐藏窗口。
