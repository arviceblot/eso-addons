#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use eso_addons_core::service::AddonService;
use std::sync::Mutex;

#[tauri::command]
async fn update(state: tauri::State<'_, AppState>) -> Result<String, ()> {
    // state.service.update().await?;
    Ok("".to_string())
}

#[tauri::command]
async fn get_installed_addon_count(state: tauri::State<'_, AppState>) -> Result<i32, ()> {
    Ok(1)
}

struct AppState {
    service: Mutex<AddonService>,
}

#[tokio::main]
async fn main() {
    // allow async outside tauri to init db connection
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    let service = AddonService::new().await;

    let state = AppState {
        service: Mutex::new(service),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![get_installed_addon_count,])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
