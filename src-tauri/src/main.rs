#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use eso_addons_api::ApiClient;

#[tauri::command]
fn update() -> String {
    let mut client = ApiClient::new("https://api.mmoui.com/v3");
    "".to_string()
}

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
