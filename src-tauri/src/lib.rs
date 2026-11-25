//! PhysioPulse desktop shell.
//!
//! - Native window with the system material and a native menu bar that sends command ids to the UI.
//! - `server` holds the only HTTP client (to the ZimaOS server over Tailscale). The session
//!   token stays in memory in Rust, never in the UI, and closing the window signs out.
//! - `messages` opens a prepared reminder in Messages, WhatsApp or email (nothing else). Desktop only.
//! - `local_ai`: chat and speech-to-text on this computer, through the app's own C++ module (`native/`: llama.cpp
//!   and whisper.cpp). Nothing leaves the computer. Desktop only.
//! - `updates`: the in-app update button. New versions come from GitHub Releases, and each one must be signed
//!   with the owner's updater key or it isn't installed (claude/RELEASING.md). Desktop only.
//! - `phone`: the staff iPhone app (phase 5) — the same core built for iOS, with the session token in the
//!   keychain behind Face ID. The window commands, menus and single-instance plugin are desktop only.

#[cfg(target_os = "macos")]
mod menu;
#[cfg(desktop)]
mod local_ai;
#[cfg(desktop)]
mod messages;
#[cfg(desktop)]
mod updates;
mod phone;
mod server;

use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri::Emitter;
#[cfg(desktop)]
use tauri::Manager;

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    /// "phone" in the iPhone app (the UI shows the phone layout), "desktop" otherwise.
    form: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo { version: env!("CARGO_PKG_VERSION"), form: if server::PHONE { "phone" } else { "desktop" } }
}

/// Light, dark or the system look for the window frame (title bar, Mica/vibrancy, native pop-ups).
/// The page itself switches with `data-theme` (src/lib/theme.ts).
#[cfg(desktop)]
#[tauri::command]
fn set_theme(window: tauri::WebviewWindow, theme: String) {
    let t = match theme.as_str() {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    };
    let _ = window.set_theme(t);
}

/// The in-window menus on Windows (Window › Minimise / Maximise, View › Full Screen, F11): audit M12.
/// Also the caption buttons the UI draws in its title bar on Windows (minimise, maximise/restore, close).
#[cfg(desktop)]
#[tauri::command]
fn window_action(window: tauri::WebviewWindow, action: String) {
    let _ = match action.as_str() {
        "minimize" => window.minimize(),
        "close" => window.close(),
        "toggle_maximize" => if window.is_maximized().unwrap_or(false) { window.unmaximize() } else { window.maximize() },
        "toggle_fullscreen" => window.set_fullscreen(!window.is_fullscreen().unwrap_or(false)),
        _ => Ok(()),
    };
}

/// Whether the window is maximised, so the caption button shows Maximise or Restore.
#[cfg(desktop)]
#[tauri::command]
fn window_maximized(window: tauri::WebviewWindow) -> bool {
    window.is_maximized().unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(desktop)]
    desktop();
    #[cfg(mobile)]
    phone_app();
}

/// The iPhone app (phase 5): no menus, no window commands; the token lives behind Face ID (`phone`).
#[cfg(mobile)]
fn phone_app() {
    tauri::Builder::default()
        .manage(server::Conn::default())
        .setup(|app| {
            server::load(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_info, server::get_server, server::set_server, server::api, server::view_file,
            server::remembered_account, server::forget_account, phone::phone_user, phone::phone_unlock, phone::phone_lock, phone::phone_sign_out])
        .run(tauri::generate_context!())
        .expect("error while running PhysioPulse");
}

#[cfg(desktop)]
fn desktop() {
    // One window only: opening the app again brings the open window to the front (a second
    // window would have its own sign-in and session). Must be the first plugin.
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(server::Conn::default())
        .manage(local_ai::Local::default());

    // The native menu bar is macOS only. On Windows the UI draws its own in-window
    // menus and handles shortcuts itself (`showMenus` in src/App.tsx), so a native
    // menu there would be a second, duplicate menu bar with clashing shortcuts.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(|app, event| {
        // Forward every custom menu id to the UI; it runs the same command table
        // the in-window menus and keyboard shortcuts use.
        let _ = app.emit("menu", event.id().as_ref().to_string());
    });

    builder
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                server::sign_out_on_close(window.app_handle());
            }
        })
        .setup(|app| {
            server::load(app.handle());
            local_ai::start(app.handle());
            let window = app.get_webview_window("main").expect("main window");
            #[cfg(target_os = "macos")]
            {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                let _ = apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, None);
            }
            #[cfg(target_os = "windows")]
            {
                // One title bar, integrated with the window (owner, 11 Nov 2026): no OS caption strip with the
                // app's name; the UI's own frosted bar is the title bar and draws the Windows caption buttons.
                // Windows 11 keeps the rounded corners, shadow, Snap (drag to an edge, Win+arrows) and resizing.
                let _ = window.set_decorations(false);
                let _ = window.set_shadow(true);
                let _ = window_vibrancy::apply_mica(&window, None);
            }
            let _ = window;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_info, set_theme, window_action, window_maximized, server::get_server, server::set_server, server::api, server::download, server::view_file, server::remembered_account, server::forget_account, messages::open_message,
            local_ai::local_ai_status, local_ai::local_ai_open_folder, local_ai::local_ai_unload, local_ai::local_chat, local_ai::local_chat_stop, local_ai::local_transcribe, local_ai::local_transcribe_stop,
            updates::update_check, updates::update_install,
            phone::phone_user, phone::phone_unlock, phone::phone_lock, phone::phone_sign_out])
        .run(tauri::generate_context!())
        .expect("error while running PhysioPulse");
}
