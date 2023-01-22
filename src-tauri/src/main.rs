#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use eso_addons_core::service::{AddonService, SearchDbAddon};
use tokio::sync::Mutex;

#[tauri::command]
async fn update(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut service = state.service.lock().await;
    service
        .update()
        .await
        .map_err(|e| e.to_string())
        .to_owned()
        .unwrap();
    Ok(())
}

#[tauri::command]
async fn install(state: tauri::State<'_, AppState>, addon_id: i32) -> Result<(), String> {
    let service = state.service.lock().await;
    service
        .install(addon_id, false)
        .await
        .expect("Install failed!");
    Ok(())
}

#[tauri::command]
async fn get_installed_addon_count(state: tauri::State<'_, AppState>) -> Result<i32, ()> {
    // let service = state.service.lock().unwrap();
    // let count = tauri::async_runtime::block_on(service.get_installed_addon_count()).map_err(|e| e.to_string());
    let service = state.service.lock().await;
    let count = service
        .get_installed_addon_count()
        .await
        .map_err(|e| e.to_string())
        .to_owned()
        .unwrap();

    Ok(count)
    // Ok(1)
}

#[tauri::command]
async fn get_installed_addons(state: tauri::State<'_, AppState>) -> Result<Vec<SearchDbAddon>, ()> {
    let service = state.service.lock().await;
    let addons = service
        .get_installed_addons()
        .await
        .map_err(|e| e.to_string())
        .unwrap();
    Ok(addons)
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
        .invoke_handler(tauri::generate_handler![
            get_installed_addon_count,
            update,
            install,
            get_installed_addons,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
