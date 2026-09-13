/// Tauri 命令桥接层 — 所有前端 ↔ 后端交互接口
use bcrypt::{hash, verify, DEFAULT_COST};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::bot::{self, BotEvent, BotState};
use crate::config::AppConfig;
use crate::cookie::{CookieVerifyResult, QrGenerateResult};
use crate::history::HistoryEntry;
use crate::paths;
use crate::video_fetcher::VideoInfo;

// ════════════════════════════════════════════════════════════════
//  机器人控制
// ════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn start_bot(state: State<'_, Arc<BotState>>) -> Result<(), String> {
    bot::try_start(state.inner())?;
    // 立即推一次统计，避免界面在首轮结束前一直显示 "—"
    let total = state.history.lock().await.total_replied();
    state
        .send_stats(total, state.rate_limiter.failure_count())
        .await;
    log::info!("机器人已启动");
    Ok(())
}

#[tauri::command]
pub async fn stop_bot(state: State<'_, Arc<BotState>>) -> Result<(), String> {
    if !state.running.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("机器人未在运行".into());
    }
    state
        .shutdown
        .store(true, std::sync::atomic::Ordering::Relaxed);
    state
        .running
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = state.event_tx.send(BotEvent::Status { running: false });
    let total = state.history.lock().await.total_replied();
    state
        .send_stats(total, state.rate_limiter.failure_count())
        .await;
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
        "history_available": state.history_available,
        "data_dir": paths::data_dir().to_string_lossy(),
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
    // 即时写入 bot_state.config，使 get_video_list / check_ollama_availability 等
    // 读取 bot_state.config 的命令立即反映新配置，无需等待主循环下一轮
    *bot_state.config.write().await = new_config.clone();
    // 日志设置立即生效（即使机器人没在运行）
    crate::logger::apply(&new_config.logging);
    // 广播通知主循环：重配 rate_limiter、记录日志，并打断 check_interval 等待
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
    let mut notes: Vec<String> = Vec::new();

    // ── config.toml ──
    let config_src = src.join("config.toml");
    if config_src.exists() {
        let dest = app_config.file_path();

        // 先校验能解析，再覆盖 —— 迁移不该毁掉一份可用的现有配置
        let parsed_ok = std::fs::read_to_string(&config_src)
            .ok()
            .and_then(|c| toml::from_str::<crate::config::RawConfig>(&c).ok())
            .is_some();

        if !parsed_ok {
            errors.push("config.toml: 解析失败，已跳过（现有配置未改动）".to_string());
        } else {
            // 备份现有配置
            if dest.exists() {
                let backup = dest.with_extension("toml.bak");
                if let Err(e) = std::fs::copy(&dest, &backup) {
                    notes.push(format!("原配置备份失败（仍会继续迁移）: {}", e));
                } else {
                    notes.push(format!("原配置已备份为 {}", backup.display()));
                }
            }
            match std::fs::copy(&config_src, &dest) {
                Err(e) => errors.push(format!("config.toml: {}", e)),
                Ok(_) => {
                    migrated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if let Ok(cfg) = app_config.reload() {
                        // 迁移进来的配置立即生效
                        *bot_state.config.write().await = cfg.clone();
                        crate::logger::apply(&cfg.logging);
                        let _ = bot_state.reload_tx.send(cfg);
                    }
                }
            }
        }
    }

    // ── history.json ──
    let history_src = src.join("history.json");
    if history_src.exists() {
        // 中转文件放在数据目录，不再污染工作目录
        let tmp = paths::resolve("history.json.migrating");
        let _ = std::fs::remove_file(&tmp);
        if let Err(e) = std::fs::copy(&history_src, &tmp) {
            errors.push(format!("history.json: {}", e));
        } else {
            let history = bot_state.history.lock().await;
            match history.import_from_json(&tmp) {
                Ok(n) => {
                    migrated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    notes.push(format!("已导入 {} 条历史记录", n));
                }
                Err(e) => errors.push(format!("history.json 导入失败: {}", e)),
            }
            drop(history);
            let _ = std::fs::remove_file(&tmp);
        }
    }

    // ── bilibili_cookie.json ──
    let cookie_src = src.join("bilibili_cookie.json");
    if cookie_src.exists() {
        let dest = paths::cookie_file();
        if let Err(e) = std::fs::copy(&cookie_src, &dest) {
            errors.push(format!("bilibili_cookie.json: {}", e));
        } else {
            migrated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut cm = bot_state.cookie_manager.lock().await;
            let _ = cm.load_from_file(&dest);
        }
    }

    // ── video_cache.json ──
    let video_cache_src = src.join("video_cache.json");
    if video_cache_src.exists() {
        let cfg = app_config.get();
        let dest = paths::resolve(&cfg.video_cache.cache_file);
        if let Err(e) = std::fs::copy(&video_cache_src, &dest) {
            errors.push(format!("video_cache.json: {}", e));
        } else {
            migrated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    Ok(serde_json::json!({
        "success": errors.is_empty(),
        "migrated_count": migrated.load(std::sync::atomic::Ordering::Relaxed),
        "errors": errors,
        "notes": notes,
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
        let _ = cm.save_to_file(&paths::cookie_file());
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
                let _ = cm.save_to_file(&paths::cookie_file());
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

/// 手动设置 Cookie。
///
/// **先校验再落盘**：早先的实现会先把 Cookie 写进文件与 config.toml，验证失败时
/// 一份原本可用的登录就已经被覆盖掉了，用户还看不出发生了什么。
/// 现在校验失败会完整恢复原有凭证，并把校验结果返回给前端。
#[tauri::command]
pub async fn set_cookie_manually(
    app_config: State<'_, AppConfig>,
    bot_state: State<'_, Arc<BotState>>,
    cookie_str: String,
    refresh_token: Option<String>,
) -> Result<CookieVerifyResult, String> {
    let mut cm = bot_state.cookie_manager.lock().await;

    let prev_cookies = cm.cookies.clone();
    let prev_refresh_token = cm.refresh_token.clone();

    cm.set_cookie_from_str(&cookie_str);
    if let Some(rt) = refresh_token {
        if !rt.trim().is_empty() {
            cm.refresh_token = rt;
        }
    }
    cm.csrf_token = cm.get_csrf_from_cookie();

    // 无 SESSDATA/bili_jct 时直接返回，不会发起网络请求
    let verify = cm.verify_cookie().await;

    if !verify.valid {
        // 回滚，保住原来能用的登录
        cm.cookies = prev_cookies;
        cm.refresh_token = prev_refresh_token;
        cm.csrf_token = cm.get_csrf_from_cookie();
        log::warn!("手动 Cookie 校验失败，已回滚到原有凭证: {}", verify.message);
        return Ok(verify);
    }

    let _ = cm.save_to_file(&paths::cookie_file());
    let cookie_saved = cm.get_cookie_str();
    let refresh_token_saved = cm.refresh_token.clone();
    let uid = verify.uid.clone();
    drop(cm);

    // 与扫码登录保持一致：同步写入 config.toml，
    // 否则「已登录」判定与依赖 uid 取视频列表的逻辑都会失效
    {
        let mut cfg = bot_state.config.write().await;
        cfg.bilibili.cookie = cookie_saved;
        cfg.bilibili.refresh_token = refresh_token_saved;
        if let Some(u) = uid {
            cfg.bilibili.uid = u;
        }
        let _ = app_config.save(cfg.clone());
    }

    Ok(verify)
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

    let policy = bot_state.rate_limiter.retry_policy();
    let config = bot_state.config.read().await;
    let videos = crate::video_fetcher::get_video_list(
        &client,
        &config.bilibili.uid,
        config.bilibili.max_video_pages,
        &paths::resolve(&config.video_cache.cache_file),
        config.video_cache.expire_time,
        policy,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(videos)
}

#[tauri::command]
pub async fn trigger_manual_check(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<String, String> {
    if !bot_state
        .running
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Err("请先启动机器人".into());
    }
    bot_state
        .manual_trigger
        .store(true, std::sync::atomic::Ordering::Relaxed);
    bot_state.send_log("INFO", "手动触发评论检查，将在当前等待结束后立即执行");
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
    let (total, items) = history.query_paginated(p, ps);

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
    let groups_raw = history.query_grouped();

    let groups: Vec<serde_json::Value> = groups_raw
        .into_iter()
        .map(|(bvid, video_title, entries)| {
            let reply_count = entries.len();
            let last_reply_time = entries.first().map(|e| e.timestamp.clone());
            let tree = build_comment_tree_from_entries(&entries);

            serde_json::json!({
                "bvid": bvid,
                "video_title": video_title,
                "reply_count": reply_count,
                "last_reply_time": last_reply_time,
                "comments": tree,
                "flat_entries": entries,
            })
        })
        .collect();

    Ok(serde_json::json!(groups))
}

fn build_comment_tree_from_entries(entries: &[HistoryEntry]) -> serde_json::Value {
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
    _all: &[HistoryEntry],
) -> serde_json::Value {
    let children = children_map.get(parent_id);
    if children.is_none() {
        return serde_json::json!([]);
    }
    children
        .unwrap()
        .iter()
        .map(|child| {
            let grandchildren = build_children_json(&child.comment_id, children_map, _all);
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

/// 按顶级评论日期分组的历史记录（日期折叠 + 树状结构）
#[tauri::command]
pub async fn get_history_by_date(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<serde_json::Value, String> {
    let history = bot_state.history.lock().await;
    let entries = history.query_all_flat();

    let roots: Vec<&HistoryEntry> = entries
        .iter()
        .filter(|e| e.depth == 0 || e.parent_id.is_none())
        .collect();

    // 构建子评论映射
    let root_children_map: std::collections::HashMap<&str, Vec<&HistoryEntry>> = entries
        .iter()
        .filter(|e| e.parent_id.is_some())
        .fold(std::collections::HashMap::new(), |mut acc, e| {
            acc.entry(e.parent_id.as_deref().unwrap())
                .or_default()
                .push(e);
            acc
        });

    // 收集 orphans（不在 root_children_map 中的非根评论）
    let orphans: Vec<&HistoryEntry> = entries
        .iter()
        .filter(|e| {
            (e.depth > 0 && e.parent_id.is_some())
                && !root_children_map.contains_key(e.comment_id.as_str())
        })
        .collect();

    // 按日期分组（从 timestamp "2026-06-13 08:38:22" 提取 "2026-06-13"）
    let mut date_groups: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        std::collections::BTreeMap::new();

    for root in &roots {
        let date = root.timestamp.chars().take(10).collect::<String>();
        let children = build_children_json(&root.comment_id, &root_children_map, &entries);
        date_groups.entry(date).or_default().push(serde_json::json!({
            "comment_id": root.comment_id,
            "user": root.user,
            "content": root.content,
            "reply_content": root.reply_content,
            "timestamp": root.timestamp,
            "depth": root.depth,
            "video_title": root.video_title,
            "bvid": root.bvid,
            "children": children,
        }));
    }

    // orphans 也按日期分配
    for orphan in &orphans {
        let date = orphan.timestamp.chars().take(10).collect::<String>();
        date_groups.entry(date).or_default().push(serde_json::json!({
            "comment_id": orphan.comment_id,
            "user": orphan.user,
            "content": orphan.content,
            "reply_content": orphan.reply_content,
            "timestamp": orphan.timestamp,
            "depth": orphan.depth,
            "video_title": orphan.video_title,
            "bvid": orphan.bvid,
            "children": [],
        }));
    }

    let groups: Vec<serde_json::Value> = date_groups
        .into_iter()
        .rev() // 最新日期在前
        .map(|(date, comments)| {
            serde_json::json!({
                "date": date,
                "comment_count": comments.len(),
                "comments": comments,
            })
        })
        .collect();

    Ok(serde_json::json!(groups))
}

#[tauri::command]
pub async fn clear_history(
    bot_state: State<'_, Arc<BotState>>,
) -> Result<(), String> {
    let history = bot_state.history.lock().await;
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
//  密码安全（bcrypt + 兼容旧 SHA-256 密码自动升级）
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
    } else if password.chars().count() < 4 {
        return Err("密码至少需要 4 个字符".into());
    } else {
        cfg.auth.enabled = true;
        let hashed = hash(password.as_bytes(), DEFAULT_COST)
            .map_err(|e| format!("密码哈希失败: {}", e))?;
        cfg.auth.password = hashed;
    }
    app_config.save(cfg).map_err(|e| e.to_string())
}

/// 验证密码。支持 bcrypt 以及旧版 SHA-256 哈希（自动升级）
#[tauri::command]
pub async fn verify_password(
    app_config: State<'_, AppConfig>,
    input: String,
) -> Result<bool, String> {
    let cfg = app_config.get();
    if !cfg.auth.enabled || cfg.auth.password.is_empty() {
        return Ok(true);
    }

    let stored = &cfg.auth.password;

    // bcrypt hash 以 $2b$ / $2a$ / $2y$ 开头
    if stored.starts_with("$2") {
        return verify(input.as_bytes(), stored)
            .map_err(|e| format!("密码验证失败: {}", e));
    }

    // 兼容旧版 SHA-256 hex（64位十六进制）
    use sha2::Digest;
    let sha256_input = sha2::Sha256::digest(input.as_bytes());
    let sha256_hex = format!("{:x}", sha256_input);
    if sha256_hex == *stored {
        // 自动升级为 bcrypt
        let upgraded = hash(input.as_bytes(), DEFAULT_COST)
            .map_err(|e| format!("密码升级失败: {}", e))?;
        let mut new_cfg = app_config.get();
        new_cfg.auth.password = upgraded;
        app_config.save(new_cfg).map_err(|e| e.to_string())?;
        log::info!("旧版 SHA-256 密码已自动升级为 bcrypt");
        return Ok(true);
    }

    Ok(false)
}

// ════════════════════════════════════════════════════════════════
//  开机自启（Windows 注册表 Run 项）
// ════════════════════════════════════════════════════════════════

/// 查询开机自启状态（以注册表为准，并同步配置文件中的记录）
#[tauri::command]
pub async fn get_autostart_status(
    app_config: State<'_, AppConfig>,
) -> Result<serde_json::Value, String> {
    let enabled = crate::autostart::is_enabled();
    let launch_command = crate::autostart::launch_command().unwrap_or_default();

    // 配置文件里的记录可能与注册表不一致（用户在任务管理器里禁用等），以注册表为准
    let mut current = app_config.get();
    if current.app.autostart != enabled {
        current.app.autostart = enabled;
        app_config.save(current).map_err(|e| e.to_string())?;
    }

    Ok(serde_json::json!({
        "enabled": enabled,
        "launch_command": launch_command,
        "supported": cfg!(windows),
    }))
}

/// 开启 / 关闭开机自启（写入或删除注册表 Run 项，并持久化到 config.toml）
#[tauri::command]
pub async fn set_autostart(
    app_config: State<'_, AppConfig>,
    bot_state: State<'_, Arc<BotState>>,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    crate::autostart::set(enabled).map_err(|e| e.to_string())?;

    // 以注册表实际状态为准（写失败或系统策略拦截时不至于撒谎）
    let actual = crate::autostart::is_enabled();
    let mut current = app_config.get();
    current.app.autostart = actual;
    app_config
        .save(current.clone())
        .map_err(|e| e.to_string())?;
    // 同步内存配置，避免 bot_state 中的副本过期
    *bot_state.config.write().await = current.clone();
    let _ = bot_state.reload_tx.send(current);

    log::info!("开机自启已{}", if actual { "开启" } else { "关闭" });

    Ok(serde_json::json!({
        "enabled": actual,
        "launch_command": crate::autostart::launch_command().unwrap_or_default(),
        "supported": cfg!(windows),
    }))
}

// ════════════════════════════════════════════════════════════════
//  数据文件（清空 / 预览）
// ════════════════════════════════════════════════════════════════

/// 清空操作涉及的**全部**文件/目录。
///
/// 只允许出现在用户数据目录内 —— 早先的实现会递归清空进程当前工作目录下
/// 除 `.exe` 以外的所有内容，在开发目录里等于把整个仓库（含 `.git`）移进回收站，
/// 装成 MSI 后则等于把安装目录清空。
fn managed_data_paths(cache_file: &str) -> Vec<PathBuf> {
    let dir = paths::data_dir();
    let mut out: Vec<PathBuf> = paths::DATA_FILES.iter().map(|n| dir.join(n)).collect();

    // 自定义的缓存文件名
    let cache_path = paths::resolve(cache_file);
    if cache_path.starts_with(&dir) && !out.contains(&cache_path) {
        out.push(cache_path);
    }

    out.push(dir.join(paths::LOG_DIR_NAME));
    out.push(dir.join(crate::single_instance::LOCK_FILE_NAME));
    out.retain(|p| p.starts_with(&dir));
    out
}

fn data_file_report(cache_file: &str) -> serde_json::Value {
    let files: Vec<serde_json::Value> = managed_data_paths(cache_file)
        .into_iter()
        .map(|p| {
            let size = if p.is_file() {
                std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            serde_json::json!({
                "path": p.to_string_lossy(),
                "exists": p.exists(),
                "size": size,
            })
        })
        .collect();

    serde_json::json!({
        "data_dir": paths::data_dir().to_string_lossy(),
        "files": files,
    })
}

/// 列出「清空数据」会处理的文件，供前端在确认框里如实展示
#[tauri::command]
pub async fn get_data_files(
    app_config: State<'_, AppConfig>,
) -> Result<serde_json::Value, String> {
    let cfg = app_config.get();
    Ok(data_file_report(&cfg.video_cache.cache_file))
}

/// 停止机器人 → 关闭数据库 → 把**数据目录内**的运行时文件移入回收站 → 退出应用。
///
/// 当没有可清理的文件时不会退出应用（前端据此显示不同提示）。
#[tauri::command]
pub async fn clear_all_data(
    app_handle: tauri::AppHandle,
    app_config: State<'_, AppConfig>,
    bot_state: State<'_, Arc<BotState>>,
) -> Result<serde_json::Value, String> {
    // 1. 停止机器人
    if bot_state
        .running
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        bot_state
            .shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        bot_state
            .running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = bot_state.event_tx.send(BotEvent::Status { running: false });
        log::info!("已停止机器人，准备清空数据");
        // 等待后台任务退出
        tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
    }

    // 2. 关闭数据库连接（WAL checkpoint 刷盘 + 释放文件句柄）
    {
        let mut history = bot_state.history.lock().await;
        history.close();
    }

    // 3. 只清理数据目录内的已知文件
    let cfg = app_config.get();
    let targets: Vec<PathBuf> = managed_data_paths(&cfg.video_cache.cache_file)
        .into_iter()
        .filter(|p| p.exists())
        .collect();

    let data_dir = paths::data_dir();
    let mut errors: Vec<String> = Vec::new();
    let mut trashed = 0u32;
    let mut trashed_paths: Vec<String> = Vec::new();

    if targets.is_empty() {
        log::info!("没有可清理的数据文件（数据目录: {:?}）", data_dir);
        return Ok(serde_json::json!({
            "trashed": 0,
            "total": 0,
            "errors": [],
            "data_dir": data_dir.to_string_lossy(),
            "files": [],
            "message": "没有可清理的数据文件",
        }));
    }

    for path in &targets {
        // 双保险：绝不越过数据目录
        if !path.starts_with(&data_dir) {
            errors.push(format!("{}: 不在数据目录内，已跳过", path.display()));
            continue;
        }
        match trash::delete(path) {
            Ok(()) => {
                trashed += 1;
                trashed_paths.push(path.to_string_lossy().to_string());
                log::info!("已移至回收站: {}", path.display());
            }
            Err(e) => {
                let msg = format!("{}: {}", path.display(), e);
                log::error!("{}", msg);
                errors.push(msg);
            }
        }
    }

    log::info!(
        "清空完成: {}/{} 项已移至回收站（数据目录: {:?}）",
        trashed,
        targets.len(),
        data_dir
    );

    let result = serde_json::json!({
        "trashed": trashed,
        "total": targets.len(),
        "errors": errors,
        "data_dir": data_dir.to_string_lossy(),
        "files": trashed_paths,
    });

    // 4. 延迟退出，让前端收到响应（只有真的清掉了东西才退出）
    if trashed > 0 {
        let handle = app_handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            handle.exit(0);
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 清理范围必须严格限制在数据目录内
    #[test]
    fn test_managed_paths_are_confined_to_data_dir() {
        let dir = paths::data_dir();
        for p in managed_data_paths("video_cache.json") {
            assert!(
                p.starts_with(&dir),
                "清理目标越界: {:?} 不在 {:?} 内",
                p,
                dir
            );
        }
    }

    /// 绝不能把工作目录 / 源码目录卷进来（历史事故）
    #[test]
    fn test_managed_paths_exclude_cwd_and_sources() {
        let paths_list = managed_data_paths("video_cache.json");
        let names: Vec<String> = paths_list
            .iter()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .collect();

        for forbidden in ["src", "src-tauri", "node_modules", ".git", "target", "package.json"] {
            assert!(
                !names.iter().any(|n| n == forbidden),
                "清理列表不应包含 {}: {:?}",
                forbidden,
                names
            );
        }

        if let Ok(cwd) = std::env::current_dir() {
            if cwd != paths::data_dir() {
                for p in &paths_list {
                    assert!(
                        !p.starts_with(&cwd) || p.starts_with(paths::data_dir()),
                        "清理目标落在工作目录内: {:?}",
                        p
                    );
                }
            }
        }
    }

    #[test]
    fn test_managed_paths_include_expected_files() {
        let dir = paths::data_dir();
        let list = managed_data_paths("video_cache.json");
        for expected in [
            dir.join("config.toml"),
            dir.join("history.db"),
            dir.join("bilibili_cookie.json"),
            dir.join("video_cache.json"),
            dir.join("logs"),
        ] {
            assert!(list.contains(&expected), "缺少 {:?}", expected);
        }
    }

    #[test]
    fn test_data_file_report_shape() {
        let report = data_file_report("video_cache.json");
        assert!(report.get("data_dir").is_some());
        let files = report["files"].as_array().expect("files 应为数组");
        assert!(!files.is_empty());
        let first = &files[0];
        assert!(first.get("path").is_some());
        assert!(first.get("exists").is_some());
        assert!(first.get("size").is_some());
    }
}
