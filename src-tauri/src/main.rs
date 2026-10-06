#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod cdc;
mod controller;
mod hid;
mod host_mouthpad;
#[cfg(target_os = "macos")]
mod keymap;
#[cfg_attr(not(windows), allow(dead_code))]
mod keymap_windows;

use controller::{Controller, Status};
use std::sync::Arc;
use tauri::{Manager, RunEvent, State, WindowEvent};

type Ctl<'a> = State<'a, Arc<Controller>>;

#[tauri::command]
fn connect(ctl: Ctl<'_>) -> Result<(), String> {
    ctl.inner().connect()
}

#[tauri::command]
fn disconnect(ctl: Ctl<'_>) {
    ctl.disconnect();
}

#[tauri::command]
fn get_status(ctl: Ctl<'_>) -> Status {
    ctl.status()
}

#[tauri::command]
fn retry_capture(ctl: Ctl<'_>) -> Status {
    ctl.inner().start_capture();
    ctl.status()
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            connect,
            disconnect,
            get_status,
            retry_capture,
            open_accessibility_settings
        ])
        .setup(|app| {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let ctl = Arc::new(Controller::new(app.handle().clone(), tx));
            app.manage(ctl.clone());
            let writer = ctl.clone();
            std::thread::Builder::new()
                .name("hid-writer".into())
                .spawn(move || writer.run_writer(rx))?;
            ctl.start_capture();
            host_mouthpad::start(ctl.clone());

            let window = app.get_webview_window("main").expect("main window");
            ctl.set_focused(window.is_focused().unwrap_or(false));
            window.on_window_event(move |event| {
                if let WindowEvent::Focused(focused) = event {
                    ctl.set_focused(*focused);
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building remote-controller")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<Arc<Controller>>().shutdown();
            }
        });
}
