// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_sign;
mod autostart;
mod bot;
mod bvid;
mod commands;
mod comment_fetcher;
mod config;
mod cookie;
mod deepseek;
mod desktop;
mod history;
mod http_client;
mod logger;
mod ollama;
mod paths;
mod rate_limiter;
mod reply;
mod single_instance;
mod video_fetcher;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::{Mutex, RwLock, broadcast};
use tauri::Manager;

use crate::bot::{BotEvent, BotState};
use crate::config::{AiProvider, AppConfig, LoggingConfig, RawConfig};
use crate::cookie::CookieManager;
use crate::history::HistoryManager;
use crate::rate_limiter::RateLimiter;

/// 在读取配置之前，先从磁盘上把 `[logging]` 段捞出来，好让启动日志不丢。
fn load_logging_config() -> LoggingConfig {
    let path = paths::config_file();
    if !path.exists() {
        return LoggingConfig::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<RawConfig>(&content) {
            Ok(cfg) => cfg.logging,
            Err(_) => LoggingConfig::default(),
        },
        Err(_) => LoggingConfig::default(),
    }
}

/// 配置是否已经「可以跑了」：B 站已登录 + AI 已配置。
///
/// 用于启动时自动开始运行机器人 —— 否则「开机自启」只会拉起一个托盘图标，
/// 机器人本身并不会工作（旧版 README 承诺过它会，属于名不副实）。
fn config_is_runnable(cfg: &RawConfig) -> bool {
    let logged_in = !cfg.bilibili.cookie.trim().is_empty()
        || !cfg.bilibili.uid.trim().is_empty()
        || paths::cookie_file().exists();

    let ai_ready = match cfg.ai.provider {
        AiProvider::Deepseek => !cfg.deepseek.api_key.trim().is_empty(),
        AiProvider::Ollama => {
            !cfg.ollama.base_url.trim().is_empty() && !cfg.ollama.model.trim().is_empty()
        }
    };

    logged_in && ai_ready
}

fn main() {
    // 1) 把旧版本遗留在工作目录 / 程序目录下的数据搬进用户数据目录。
    //    必须在读取配置之前执行，否则会读到一份"空配置"。
    paths::migrate_legacy_files();

    // 2) 日志：写文件 + 可选输出到 stderr，级别取自 [logging]。
    //    注意正式版没有控制台（windows_subsystem = "windows"），
    //    所以日志文件是唯一的排查手段。
    logger::init(&load_logging_config());

    log::info!(
        "启动 BiliCommentBot-RS，数据目录: {:?}",
        paths::data_dir()
    );

    tauri::Builder::default()
        // 系统托盘：左键显示主窗口，右键菜单提供「显示主窗口 / 退出程序」
        .system_tray(desktop::tray())
        .on_system_tray_event(desktop::on_tray_event)
        // 关闭主窗口时按配置询问 / 最小化到托盘 / 直接退出
        .on_window_event(desktop::on_window_event)
        .setup(|app| {
            // 单实例保护：两个实例会共用一个 history.db / cookie，互相抢着回复
            if !single_instance::acquire() {
                log::warn!("检测到已有实例在运行，本次启动将退出");
                let window = app.get_window("main");
                let app_handle = app.handle();
                tauri::api::dialog::blocking::message(
                    window.as_ref(),
                    "BiliCommentBot-RS",
                    "程序已经在运行了。\n\n请查看系统托盘图标（右下角），或从托盘菜单选择「显示主窗口」。",
                );
                app_handle.exit(0);
                return Ok(());
            }

            let app_config = AppConfig::new();
            let cfg = app_config.get();
            let rl = &cfg.rate_limit;
            let autostart_enabled = cfg.app.autostart;
            let start_minimized = autostart::started_minimized();

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

            // 尝试加载已有 Cookie（数据目录）
            let cookie_path = paths::cookie_file();
            if cookie_path.exists() {
                let _ = cookie_mgr.load_from_file(&cookie_path);
            }
            // 如果配置中有 cookie 字符串，以配置为准覆盖
            if !cfg.bilibili.cookie.is_empty() {
                cookie_mgr.set_cookie_from_str(&cfg.bilibili.cookie);
                cookie_mgr.refresh_token = cfg.bilibili.refresh_token.clone();
                cookie_mgr.csrf_token = cookie_mgr.get_csrf_from_cookie();
            }

            // 创建日志广播通道
            let (event_tx, _) = broadcast::channel::<BotEvent>(1024);

            // 创建配置重载通道
            let (reload_tx, _) = broadcast::channel::<RawConfig>(8);

            // 历史管理器（数据目录下的 history.db；打不开时降级而不是 panic）
            let history = HistoryManager::new(&paths::history_db());
            let history_available = history.available();
            if !history_available {
                log::error!(
                    "历史数据库不可用：{:?}。机器人将被禁止启动，以免重复回复同一批评论。",
                    history.db_path()
                );
            }

            // 频率控制器
            let rate_limiter = RateLimiter::new(
                rl.min_request_interval,
                rl.max_retries,
                rl.retry_delay,
            );

            let bot_state = Arc::new(BotState {
                config: RwLock::new(cfg.clone()),
                history: Mutex::new(history),
                cookie_manager: Mutex::new(cookie_mgr),
                running: AtomicBool::new(false),
                start_time: Mutex::new(None),
                last_check: Mutex::new(None),
                event_tx: event_tx.clone(),
                reload_tx,
                shutdown: AtomicBool::new(false),
                manual_trigger: AtomicBool::new(false),
                rate_limiter,
                history_available,
                last_cookie_check: Mutex::new(None),
            });

            // 后台任务：广播 BotEvent 到 Tauri 窗口
            let window = app.get_window("main");
            let mut event_rx = event_tx.subscribe();
            if let Some(window) = window {
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
            } else {
                log::error!("未找到主窗口 main，前端事件推送不可用");
            }

            app.manage(app_config);
            app.manage(bot_state.clone());
            app.manage(desktop::DesktopState::default());

            // 窗口显示策略：
            //  · 开机自启（命令行带 --minimized）→ 保持隐藏，静默运行在系统托盘
            //  · 正常启动 → 显示并聚焦主窗口
            // tauri.conf.json 中窗口默认 visible=false，由这里决定是否显示，
            // 这样开机自启时不会闪出窗口。
            match app.get_window("main") {
                Some(window) => {
                    if start_minimized {
                        let _ = window.hide();
                        log::info!("检测到 {} 参数，主窗口保持隐藏（系统托盘运行）", autostart::MINIMIZED_ARG);
                    } else {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                None => log::warn!("未找到主窗口 main，无法设置显示状态"),
            }

            // 配置里记着已开启开机自启时，刷新注册表项以指向当前 exe 路径
            autostart::sync_if_enabled(autostart_enabled);

            // 启动即运行：配置完整且允许自动启动时，直接开始工作，
            // 否则「开机自启」只会拉起托盘图标而机器人并不干活。
            if cfg.app.auto_start_bot {
                if !config_is_runnable(&cfg) {
                    log::info!("配置尚不完整（未登录或未配置 AI），不自动启动机器人");
                } else {
                    match bot::try_start(&bot_state) {
                        Ok(()) => log::info!("配置完整，已自动启动机器人"),
                        Err(e) => log::warn!("自动启动机器人失败: {}", e),
                    }
                }
            } else {
                log::info!("[app] auto_start_bot = false，不自动启动机器人");
            }

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
            commands::get_history_by_date,
            commands::clear_history,
            commands::check_ollama_availability,
            commands::list_ollama_models,
            commands::set_password,
            commands::verify_password,
            commands::clear_all_data,
            commands::get_data_files,
            commands::get_autostart_status,
            commands::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
