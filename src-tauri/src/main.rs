// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_sign;
mod bvid;
mod config;
mod cookie;
mod decompress;
mod http_client;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    tauri::Builder::default()
        .manage(config::AppConfig::new())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
