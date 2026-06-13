// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_sign;
mod bot;
mod bvid;
mod commands;
mod comment_fetcher;
mod config;
mod cookie;
mod decompress;
mod deepseek;
mod history;
mod http_client;
mod ollama;
mod rate_limiter;
mod reply;
mod video_fetcher;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::{Mutex, RwLock, broadcast};
use tauri::Manager;

use crate::bot::{BotEvent, BotState};
use crate::config::{AppConfig, RawConfig};
use crate::cookie::CookieManager;
use crate::history::HistoryManager;
use crate::rate_limiter::RateLimiter;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let app_config = AppConfig::new();
            let cfg = app_config.get();
            let rl = &cfg.rate_limit;

            // 初始化 CookieManager（含10秒超时防止挂起）
            let mut cookie_mgr =
                CookieManager::from_client(reqwest::Client::builder()
                    .cookie_store(true)
                    .gzip(true)
                    .deflate(true)
                    .brotli(true)
                    .timeout(std::time::Duration::from_secs(10))
                    .connect_timeout(std::time::Duration::from_secs(5))
                    .build()
                    .expect("Failed to build cookie HTTP client"));

            // 尝试加载已有 Cookie
            let cookie_path = std::path::PathBuf::from(crate::cookie::DEFAULT_COOKIE_FILE);
            if cookie_path.exists() {
                let _ = cookie_mgr.load_from_file(&cookie_path);
            }
            // 如果配置中有 cookie 字符串
            if !cfg.bilibili.cookie.is_empty() {
                cookie_mgr.set_cookie_from_str(&cfg.bilibili.cookie);
                cookie_mgr.refresh_token = cfg.bilibili.refresh_token.clone();
                cookie_mgr.csrf_token = cookie_mgr.get_csrf_from_cookie();
            }

            // 创建日志广播通道
            let (event_tx, _) = broadcast::channel::<BotEvent>(1024);

            // 创建配置重载通道
            let (reload_tx, _) = broadcast::channel::<RawConfig>(8);

            // 历史管理器
            let history = HistoryManager::new(std::path::PathBuf::from("history.json"));

            // 频率控制器
            let rate_limiter = RateLimiter::new(
                rl.min_request_interval,
                rl.max_retries,
                rl.retry_delay,
            );

            let bot_state = Arc::new(BotState {
                config: RwLock::new(cfg),
                history: Mutex::new(history),
                cookie_manager: Mutex::new(cookie_mgr),
                running: AtomicBool::new(false),
                start_time: Mutex::new(None),
                last_check: Mutex::new(None),
                event_tx: event_tx.clone(),
                reload_tx,
                shutdown: AtomicBool::new(false),
                rate_limiter,
            });

            // 后台任务：广播 BotEvent 到 Tauri 窗口
            let window = app.get_window("main").unwrap();
            let mut event_rx = event_tx.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match event_rx.recv().await {
                        Ok(event) => {
                            let payload = serde_json::to_value(&event).unwrap_or_default();
                            let _ = window.emit("bot-event", payload);
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("事件通道滞后 {} 条消息", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            app.manage(app_config);
            app.manage(bot_state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_bot,
            commands::stop_bot,
            commands::get_bot_status,
            commands::get_config,
            commands::save_config,
            commands::migrate_from_old_project,
            commands::generate_qrcode,
            commands::poll_qr_login,
            commands::verify_cookie,
            commands::refresh_cookie,
            commands::set_cookie_manually,
            commands::get_video_list,
            commands::trigger_manual_check,
            commands::get_history,
            commands::get_history_grouped,
            commands::clear_history,
            commands::check_ollama_availability,
            commands::list_ollama_models,
            commands::set_password,
            commands::verify_password,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
