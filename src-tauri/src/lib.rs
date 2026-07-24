// Winux-Kate desktop shell — backend entry assembly.

mod debug_log;
mod shell;
mod state;
mod window_manager;
mod hotkey;
mod pty;
mod system;
mod shortcuts;
mod apps;
mod config;
mod commands;

use state::AppState;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    debug_log::init();
    debug_log::dlog("boot", "run() start");

    // Winux-Kate replaces the Windows shell: terminate the legacy explorer.exe
    // (taskbar/desktop) BEFORE creating our window, so the desktop is already
    // gone when Kate appears. Explorer is restored on exit as a safety net.
    // In release builds, always take over the shell. In debug builds, only
    // when --shell flag or WINUX_SHELL_MODE=1 is set (dev safety).
    let should_kill = if cfg!(debug_assertions) {
        shell::should_take_over_shell()
    } else {
        true
    };
    if should_kill {
        debug_log::dlog("boot", "shell takeover: killing explorer (blocking)");
        // Register a panic hook that restores explorer.exe on crash, so a
        // panic doesn't leave the system without a shell.
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = std::process::Command::new("explorer.exe").spawn();
            default_hook(info);
        }));
        // Kill explorer synchronously and wait until every explorer.exe
        // process is gone before entering the program.
        shell::kill_explorer_blocking(3000);
        debug_log::dlog(
            "boot",
            format!("explorer dead: {}", !shell::explorer_running()),
        );
    } else {
        debug_log::dlog("boot", "shell takeover skipped (debug, no --shell)");
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            // shell
            commands::install_shell,
            commands::uninstall_shell,
            commands::kill_explorer,
            commands::restore_explorer,
            commands::is_running_as_shell,
            // window manager
            commands::report_slot_rects,
            commands::hide_all_external,
            commands::get_current_page,
            commands::set_current_page,
            commands::launch_and_attach,
            commands::detach_window,
            // pty
            commands::pty_spawn,
            commands::pty_write,
            commands::pty_resize,
            commands::pty_kill,
            // system
            commands::system_status,
            commands::set_volume,
            commands::set_mute,
            commands::set_brightness,
            // shortcuts
            commands::list_shortcuts,
            commands::launch_app,
            commands::close_app,
            commands::adopt_existing_windows,
            commands::list_desktop_windows,
            commands::focus_desktop_window,
            // ide
            commands::ide_new,
            commands::ide_cycle,
            commands::ide_set_active,
            commands::ide_list,
            commands::ide_close,
            // im
            commands::im_launch,
            commands::im_toggle,
            commands::im_set_paths,
            // fs
            commands::list_dir,
            commands::read_file,
            commands::write_file,
            // detection
            commands::detect_apps,
            // custom pages
            commands::list_custom_pages,
            commands::add_custom_page,
            commands::remove_custom_page,
            commands::launch_custom_page,
            commands::launch_custom_new,
            commands::cycle_custom,
            commands::set_custom_active,
            commands::close_custom,
            commands::custom_state,
            // shell control
            commands::quit_app,
            commands::hide_overlays,
            commands::kill_im_processes,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            // Start the global low-level keyboard hook (Ctrl+Tab / Ctrl+Shift+Tab).
            hotkey::start(handle);

            // Watchdog: keep the current page's overlay windows on top and restore
            // any the user minimized (no taskbar to recover them). Does NOT resize/
            // reposition, so users can still enlarge windows. Also discovers IM
            // main windows that appear after the login window.
            let watchdog = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(800));
                let state = watchdog.state::<AppState>();
                if window_manager::prune_dead_windows(&state) {
                    let _ = watchdog.emit("ide-active-changed", ());
                }
                window_manager::discover_im_windows(&state);
                window_manager::discover_custom_windows(&state);
                window_manager::pin_overlays(&state);
            });

            // Capture the main window's native HWND for reparenting external windows.
            if let Some(win) = app.get_webview_window("main") {
                if let Ok(h) = win.hwnd() {
                    let main_hwnd = state::hwnd_from_usize(h.0 as usize);
                    app.state::<AppState>().set_main_hwnd(main_hwnd);
                    debug_log::dlog("boot", format!("main hwnd = {:#x}", h.0 as usize));
                    // Cover the full primary monitor (including the old taskbar area).
                    window_manager::set_main_fullscreen(main_hwnd);
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Resized(_) = event {
                // Notify frontend to recompute slot rects after a resize.
                let _ = window.emit("wm-resize", ());
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // On exit: return embedded windows to the normal desktop, then restore explorer.
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            let state = handle.state::<AppState>();
            window_manager::release_all(&state);
            let _ = shell::restore_explorer();
        }
    });
}
