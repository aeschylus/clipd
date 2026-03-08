// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use clipd_core::config::Config;

fn main() {
    // Initialize logging early
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("clipd_app=info".parse().unwrap())
                .add_directive("clipd_core=info".parse().unwrap()),
        )
        .with_target(false)
        .compact()
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            setup_app(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_clips,
            commands::search_clips,
            commands::get_clip,
            commands::paste_clip,
            commands::delete_clip,
            commands::toggle_pin,
            commands::get_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running clipd app");
}

fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Load config to pass around
    let config = Config::load().unwrap_or_default();

    // Read the configured hotkey (default: CmdOrCtrl+Shift+V)
    let hotkey = Shortcut::new(
        Some(Modifiers::SUPER | Modifiers::SHIFT),
        Code::KeyV,
    );

    // Register global shortcut — toggle panel visibility
    let app_handle = app.handle().clone();
    app.global_shortcut().on_shortcut(hotkey, move |_app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            toggle_panel(&app_handle);
        }
    })?;

    // Build tray menu
    let show_item = MenuItem::with_id(app, "show", "Show clipd", true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit clipd", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show_item, &separator, &quit_item])?;

    // Build tray icon
    TrayIconBuilder::with_id("tray")
        .tooltip("clipd — clipboard history")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => toggle_panel(app),
            "quit" => {
                tracing::info!("quitting clipd");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;

    // Start the clipboard monitoring daemon in a background thread
    let db_path = config.db_path.clone();
    let config_clone = config.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("building tokio runtime for daemon");
        tracing::info!(db = %db_path.display(), "starting embedded clipboard daemon");
        if let Err(e) = rt.block_on(clipd_core::daemon::run(config_clone)) {
            tracing::error!("daemon error: {:#}", e);
        }
    });

    // On macOS, hide the Dock icon (LSUIElement handles this via Info.plist,
    // but we set it programmatically as well for robustness)
    #[cfg(target_os = "macos")]
    set_activation_policy_accessory();

    tracing::info!("clipd app setup complete");
    Ok(())
}

/// Toggle the panel window visibility.
fn toggle_panel<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("clipd-panel") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            // Emit an event so the UI can refresh its clip list before showing
            let _ = app.emit("refresh-clips", ());
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

/// On macOS, set the activation policy to Accessory (no Dock icon, no menu bar).
#[cfg(target_os = "macos")]
fn set_activation_policy_accessory() {
    use objc::{msg_send, class, sel, sel_impl};
    unsafe {
        let cls = class!(NSApplication);
        let app: *mut objc::runtime::Object = msg_send![cls, sharedApplication];
        // NSApplicationActivationPolicyAccessory = 1
        let _: () = msg_send![app, setActivationPolicy: 1i64];
    }
}
