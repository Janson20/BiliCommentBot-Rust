/// Tauri 命令桥接层 — 所有前端 ↔ 后端交互接口
use sha2::Digest;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::bot::{self, BotEvent, BotState};
use crate::config::AppConfig;
use crate::cookie::QrGenerateResult;
use crate::history::HistoryEntry;
use crate::video_fetcher::VideoInfo;

// ════════════════════════════════════════════════════════════════
//  机器人控制
// ════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn start_bot(
    state: State<'_, Arc<BotState>>,
) -> Result<(), String> {
    if state.running.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("机器人已在运行中".into());
    }
    state.shutdown.store(false, std::sync::atomic::Ordering::Relaxed);
    state.running.store(true, std::sync::atomic::Ordering::Relaxed);
    bot::start_bot(state.inner().clone());
    let _ = state.event_tx.send(BotEvent::Status { running: true });
    log::info!("机器人已启动");
    Ok(())
}

#[tauri::command]
pub async fn stop_bot(
    state: State<'_, Arc<BotState>>,
) -> Result<(), String> {
    if !state.running.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("机器人未在运行".into());
    }
    state.shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    state.running.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = state.event_tx.send(BotEvent::Status { running: false });
    log::info!("机器人已停止");
    Ok(())
}

#[tauri::command]
pub async fn get_bot_status(
    state: State<'_, Arc<BotState>>,
) -> Result<serde_json::Value, String> {
    let history = state.history.lock().await;
    Ok(serde_json::json!({
        "running": state.running.load(std::sync::atomic::Ordering::Relaxed),
        "total_replied": history.total_replied(),
        "start_time": state.start_time.lock().await.clone(),
        "last_check": state.last_check.lock().await.clone(),
        "consecutive_failures": state.rate_limiter.failure_count(),
    }))
}

// ════════════════════════════════════════════════════════════════
//  配置管理
// ════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_config(
    app_config: State<'_, AppConfig>,
) -> Result<crate::config::RawConfig, String> {
    Ok(app_config.get())
}

#[tauri::command]
pub async fn save_config(
    app_config: State<'_, AppConfig>,
    bot_state: State<'_, Arc<BotState>>,
    new_config: crate::config::RawConfig,
) -> Result<(), String> {
    app_config.save(new_config.clone()).map_err(|e| e.to_string())?;
    let _ = bot_state.reload_tx.send(new_config);
    Ok(())
}

/// 从旧版 Python 项目文件夹迁移配置和数据
#[tauri::command]
pub async fn migrate_from_old_project(
    app_config: State<'_, AppConfig>,
    bot_state: State<'_, Arc<BotState>>,
    old_project_dir: String,
) -> Result<serde_json::Value, String> {
    let src = PathBuf::from(&old_project_dir);
    let migrated = std::sync::atomic::AtomicU32::new(0);
    let mut errors = Vec::new();

    let config_src = src.join("config.toml");
    if config_src.exists() {
        let dest = app_config.file_path();
        if let Err(e) = std::fs::copy(&config_src, &dest) {
            errors.push(format!("config.toml: {}", e));
        } else {
            migrated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if let Ok(cfg) = app_config.reload() {
                let _ = bot_state.reload_tx.send(cfg);
            }
        }
    }

    let history_src = src.join("history.json");
    if history_src.exists() {
        let dest = PathBuf::from("history.json");
        if let Err(e) = std::fs::copy(&history_src, &dest) {
            errors.push(format!("history.json: {}", e));
        } else {
            migrated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut history = bot_state.history.lock().await;
            let new_hm = crate::history::HistoryManager::new(dest);
            *history = new_hm;
        }
    }

    let cookie_src = src.join("bilibili_cookie.json");
    if cookie_src.exists() {
        let dest = PathBuf::from(crate::cookie::DEFAULT_COOKIE_FILE);
        if let Err(e) = std::fs::copy(&cookie_src, &dest) {
            errors.push(format!("bilibili_cookie.json: {}", e));
        } else {
            migrated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut cm) = bot_state.cookie_manager.try_lock() {
                let _ = cm.load_from_file(&dest);
            } else {
                let mut cm = bot_state.cookie_manager.lock().await;
                let _ = cm.load_from_file(&dest);
            }
        }
    }

    let video_cache_src = src.join("video_cache.json");
    if video_cache_src.exists() {
        let dest = PathBuf::from("video_cache.json");
        if let Err(e) = std::fs::copy(&video_cache_src, &dest) {
            errors.push(format!("video_cache.json: {}", e));
        } else {
            migrated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    Ok(serde_json::json!({
        "success": true,
        "migrated_count": migrated.load(std::sync::atomic::Ordering::Relaxed),
        "errors": errors,
    }))
}

// ════════════════════════════════════════════════════════════════
//  Cookie / 登录
// ════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn generate_qrcode(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<QrGenerateResult, String> {
    let cm = bot_state.cookie_manager.lock().await;
    cm.generate_qrcode().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn poll_qr_login(
    app_config: State<'_, AppConfig>,
    bot_state: State<'_, Arc<BotState>>,
    qrcode_key: String,
) -> Result<crate::cookie::QrPollResult, String> {
    let mut cm = bot_state.cookie_manager.lock().await;
    let result = cm.poll_qr_login(&qrcode_key)
        .await
        .map_err(|e| e.to_string())?;

    // 扫码成功后自动保存 Cookie + refresh_token + 更新 config.toml
    if result.code == 0 && !result.cookies.is_empty() {
        cm.cookies = result.cookies.clone();
        if let Some(ref rt) = result.refresh_token {
            cm.refresh_token = rt.clone();
        }
        cm.csrf_token = cm.get_csrf_from_cookie();
        let cookie_str = cm.get_cookie_str();
        let refresh_token = cm.refresh_token.clone();
        let _ = cm.save_to_file(&PathBuf::from(crate::cookie::DEFAULT_COOKIE_FILE));
        log::info!("扫码Cookie已保存，共 {} 条", result.cookies.len());

        // 验证并获取 UID
        let uid = {
            let verify = cm.verify_cookie().await;
            if verify.valid { verify.uid } else { None }
        };
        drop(cm);

        // 更新 bot_state 内存配置
        {
            let mut cfg = bot_state.config.write().await;
            cfg.bilibili.cookie = cookie_str;
            cfg.bilibili.refresh_token = refresh_token;
            if let Some(u) = uid {
                cfg.bilibili.uid = u;
            }
            // 持久化到 config.toml
            let _ = app_config.save(cfg.clone());
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn verify_cookie(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<crate::cookie::CookieVerifyResult, String> {
    let cm = bot_state.cookie_manager.lock().await;
    Ok(cm.verify_cookie().await)
}

#[tauri::command]
pub async fn refresh_cookie(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<serde_json::Value, String> {
    let mut cm = bot_state.cookie_manager.lock().await;
    match cm.refresh_cookie().await {
        Ok((success, msg, new_token)) => {
            if success {
                let _ = cm.save_to_file(
                    &PathBuf::from(crate::cookie::DEFAULT_COOKIE_FILE),
                );
            }
            Ok(serde_json::json!({
                "success": success,
                "message": msg,
                "refresh_token": new_token,
            }))
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn set_cookie_manually(
    bot_state: State<'_, Arc<BotState>>,
    cookie_str: String,
    refresh_token: Option<String>,
) -> Result<(), String> {
    let mut cm = bot_state.cookie_manager.lock().await;
    cm.set_cookie_from_str(&cookie_str);
    if let Some(rt) = refresh_token {
        cm.refresh_token = rt;
    }
    cm.csrf_token = cm.get_csrf_from_cookie();
    let _ = cm.save_to_file(&PathBuf::from(crate::cookie::DEFAULT_COOKIE_FILE));
    Ok(())
}

// ════════════════════════════════════════════════════════════════
//  视频 & 评论
// ════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_video_list(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<Vec<VideoInfo>, String> {
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .gzip(true)
        .deflate(true)
        .brotli(true)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let config = bot_state.config.read().await;
    let videos = crate::video_fetcher::get_video_list(
        &client,
        &config.bilibili.uid,
        config.bilibili.max_video_pages,
        &PathBuf::from(&config.video_cache.cache_file),
        config.video_cache.expire_time,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(videos)
}

#[tauri::command]
pub async fn trigger_manual_check(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<String, String> {
    if !bot_state.running.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("请先启动机器人".into());
    }
    bot_state.send_log("INFO", "手动触发评论检查");
    Ok("已触发".into())
}

// ════════════════════════════════════════════════════════════════
//  历史记录
// ════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_history(
    bot_state: State<'_, Arc<BotState>>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<serde_json::Value, String> {
    let history = bot_state.history.lock().await;
    let p = page.unwrap_or(1).max(1);
    let ps = page_size.unwrap_or(50).min(200);
    let total = history.entries.len() as u32;
    let start = ((p - 1) * ps) as usize;
    let end = (start + ps as usize).min(history.entries.len());
    let items: Vec<&HistoryEntry> = history.entries[start..end].iter().collect();

    Ok(serde_json::json!({
        "total": total,
        "page": p,
        "page_size": ps,
        "items": items,
    }))
}

/// 按视频分组的历史记录（卡片视图用）
#[tauri::command]
pub async fn get_history_grouped(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<serde_json::Value, String> {
    let history = bot_state.history.lock().await;
    let mut map: std::collections::BTreeMap<String, Vec<&HistoryEntry>> = std::collections::BTreeMap::new();
    for entry in history.entries.iter().rev() {
        let key = &entry.bvid;
        map.entry(key.clone()).or_default().push(entry);
    }

    let groups: Vec<serde_json::Value> = map
        .into_iter()
        .map(|(bvid, entries)| {
            // 构建评论树
            let tree = build_comment_tree_from_entries(&entries);

            serde_json::json!({
                "bvid": bvid,
                "video_title": entries.first().map(|e| &e.video_title).unwrap_or(&String::new()),
                "reply_count": entries.len(),
                "last_reply_time": entries.first().map(|e| &e.timestamp),
                "comments": tree,
                "flat_entries": entries,
            })
        })
        .collect();

    Ok(serde_json::json!(groups))
}

fn build_comment_tree_from_entries(entries: &[&HistoryEntry]) -> serde_json::Value {
    // 简化：按 root_id 分组，主评论为一级，子评论嵌套
    let mut roots: Vec<serde_json::Value> = Vec::new();
    let children_map: std::collections::HashMap<&str, Vec<&HistoryEntry>> = entries
        .iter()
        .filter(|e| e.parent_id.as_deref().is_some())
        .fold(std::collections::HashMap::new(), |mut acc, e| {
            acc.entry(e.parent_id.as_deref().unwrap()).or_default().push(e);
            acc
        });

    for entry in entries.iter().filter(|e| e.depth == 0 || e.parent_id.is_none()) {
        let children = build_children_json(&entry.comment_id, &children_map, entries);
        roots.push(serde_json::json!({
            "comment_id": entry.comment_id,
            "user": entry.user,
            "content": entry.content,
            "reply_content": entry.reply_content,
            "timestamp": entry.timestamp,
            "depth": entry.depth,
            "children": children,
        }));
    }

    // 如果没找到根评论，把 orphans 也放进去
    if roots.is_empty() {
        for entry in entries.iter() {
            if !children_map.contains_key(entry.comment_id.as_str()) {
                roots.push(serde_json::json!({
                    "comment_id": entry.comment_id,
                    "user": entry.user,
                    "content": entry.content,
                    "reply_content": entry.reply_content,
                    "timestamp": entry.timestamp,
                    "depth": entry.depth,
                    "children": [],
                }));
            }
        }
    }

    serde_json::json!(roots)
}

fn build_children_json(
    parent_id: &str,
    children_map: &std::collections::HashMap<&str, Vec<&HistoryEntry>>,
    all: &[&HistoryEntry],
) -> serde_json::Value {
    let children = children_map.get(parent_id);
    if children.is_none() {
        return serde_json::json!([]);
    }
    children
        .unwrap()
        .iter()
        .map(|child| {
            let grandchildren = build_children_json(&child.comment_id, children_map, all);
            serde_json::json!({
                "comment_id": child.comment_id,
                "user": child.user,
                "content": child.content,
                "reply_content": child.reply_content,
                "timestamp": child.timestamp,
                "depth": child.depth,
                "children": grandchildren,
            })
        })
        .collect::<Vec<_>>()
        .into()
}

#[tauri::command]
pub async fn clear_history(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<(), String> {
    let mut history = bot_state.history.lock().await;
    history.clear();
    Ok(())
}

// ════════════════════════════════════════════════════════════════
//  Ollama
// ════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn check_ollama_availability(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<bool, String> {
    let config = bot_state.config.read().await;
    crate::ollama::check_availability(&config.ollama.base_url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_ollama_models(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<Vec<String>, String> {
    let config = bot_state.config.read().await;
    crate::ollama::list_models(&config.ollama.base_url)
        .await
        .map_err(|e| e.to_string())
}

// ════════════════════════════════════════════════════════════════
//  密码安全
// ════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn set_password(
    app_config: State<'_, AppConfig>,
    password: String,
) -> Result<(), String> {
    let mut cfg = app_config.get();
    if password.is_empty() {
        cfg.auth.enabled = false;
        cfg.auth.password = String::new();
    } else {
        cfg.auth.enabled = true;
        let hash = sha2::Sha256::digest(password.as_bytes());
        cfg.auth.password = format!("{:x}", hash);
    }
    app_config.save(cfg).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn verify_password(
    app_config: State<'_, AppConfig>,
    input: String,
) -> Result<bool, String> {
    let cfg = app_config.get();
    if !cfg.auth.enabled || cfg.auth.password.is_empty() {
        return Ok(true);
    }
    let hash = sha2::Sha256::digest(input.as_bytes());
    Ok(format!("{:x}", hash) == cfg.auth.password)
}
