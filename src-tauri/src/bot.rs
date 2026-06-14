/// 机器人主循环 — 评论处理编排器
///
/// 整合所有业务模块：频率控制、视频获取、评论获取、AI生成、回复、点赞
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, RwLock, broadcast};

use crate::comment_fetcher::{self, Comment};
use crate::config::{AiProvider, RawConfig};
use crate::cookie::CookieManager;
use crate::deepseek;
use crate::history::HistoryManager;
use crate::http_client;
use crate::ollama;
use crate::rate_limiter::RateLimiter;
use crate::reply;
use crate::video_fetcher::{self, VideoInfo};

/// 日志条目（推送到前端）
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub time: String,
    pub level: String,
    pub msg: String,
}

/// 统计信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct BotStats {
    pub running: bool,
    pub total_replied: u64,
    pub start_time: Option<String>,
    pub last_check: Option<String>,
    pub consecutive_failures: u32,
}

/// 事件消息（前端可接收的推送事件类型）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum BotEvent {
    #[serde(rename = "log")]
    Log(LogEntry),
    #[serde(rename = "stats")]
    Stats(BotStats),
    #[serde(rename = "history")]
    History(crate::history::HistoryEntry),
    #[serde(rename = "video_list")]
    VideoList { count: usize, videos: Vec<VideoInfo> },
    #[serde(rename = "status")]
    Status { running: bool },
}

/// 机器人共享状态（线程安全，Arc 包裹供 Tauri State + 后台任务共享）
pub struct BotState {
    pub config: RwLock<RawConfig>,
    pub history: Mutex<HistoryManager>,
    pub cookie_manager: Mutex<CookieManager>,
    pub running: AtomicBool,
    pub start_time: Mutex<Option<String>>,
    pub last_check: Mutex<Option<String>>,
    pub event_tx: broadcast::Sender<BotEvent>,
    /// 通知主循环热更新配置
    pub reload_tx: broadcast::Sender<RawConfig>,
    pub shutdown: AtomicBool,
    pub rate_limiter: RateLimiter,
}

impl BotState {
    pub fn send_log(&self, level: &str, msg: &str) {
        let entry = LogEntry {
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: level.to_string(),
            msg: msg.to_string(),
        };
        let _ = self.event_tx.send(BotEvent::Log(entry));
    }

    pub fn send_stats(&self, total_replied: u64, consecutive_failures: u32) {
        let stats = BotStats {
            running: self.running.load(Ordering::Relaxed),
            total_replied,
            start_time: self.start_time.blocking_lock().clone(),
            last_check: self.last_check.blocking_lock().clone(),
            consecutive_failures,
        };
        let _ = self.event_tx.send(BotEvent::Stats(stats));
    }
}

/// 启动机器人后台任务（独立的 tokio task）
pub fn start_bot(state: Arc<BotState>) {
    let state = state.clone();
    tokio::spawn(async move {
        bot_main_loop(state).await;
    });
}

async fn bot_main_loop(state: Arc<BotState>) {
    *state.start_time.lock().await = Some(
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    );

    let http_client = reqwest::Client::builder()
        .cookie_store(true)
        .gzip(true)
        .deflate(true)
        .brotli(true)
        .user_agent(http_client::random_web_ua())
        .build()
        .expect("Failed to build HTTP client");

    let mut reload_rx = state.reload_tx.subscribe();

    let mut round = 0u64;

    loop {
        if state.shutdown.load(Ordering::Relaxed) {
            state.send_log("INFO", "收到退出信号，主循环结束");
            break;
        }

        round += 1;
        state.send_log("DEBUG", &format!("==== 第 {} 轮检查开始 ====", round));

        // 检查配置热更新
        if let Ok(new_config) = reload_rx.try_recv() {
            let rl = &new_config.rate_limit;
            state.rate_limiter.reconfigure(
                rl.min_request_interval,
                rl.max_retries,
                rl.retry_delay,
            );
            *state.config.write().await = new_config;
            state.send_log("INFO", "配置已热更新");
        }

        let config = state.config.read().await.clone();

        state.send_log("DEBUG", &format!(
            "当前配置: AI={:?}, reply_enabled={}, max_process={}, dry_run={}, check_interval={}s",
            config.ai.provider, config.reply.enabled, config.reply.max_process, config.reply.dry_run, config.bilibili.check_interval
        ));

        *state.last_check.lock().await = Some(
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        );

        // Cookie 自动刷新
        if config.bilibili.auto_refresh_cookie {
            let mut cm = state.cookie_manager.lock().await;
            if let Ok((refreshed, msg)) = cm.auto_refresh_if_needed().await {
                if refreshed {
                    state.send_log("INFO", &format!("Cookie已自动刷新: {}", msg));
                }
                let _ = cm.save_to_file(
                    &std::path::PathBuf::from(crate::cookie::DEFAULT_COOKIE_FILE),
                );
            }
        }

        // 获取视频列表
        let videos = match video_fetcher::get_video_list(
            &http_client,
            &config.bilibili.uid,
            config.bilibili.max_video_pages,
            &std::path::PathBuf::from(&config.video_cache.cache_file),
            config.video_cache.expire_time,
        )
        .await
        {
            Ok(v) => {
                let count = v.len();
                state.send_log("INFO", &format!("获取到 {} 个视频", count));
                let _ = state.event_tx.send(BotEvent::VideoList {
                    count,
                    videos: v.clone(),
                });
                v
            }
            Err(e) => {
                state.send_log("ERROR", &format!("获取视频列表失败: {}", e));
                continue;
            }
        };

        let mut videos_checked = 0u32;

        for video in &videos {
            if state.shutdown.load(Ordering::Relaxed) {
                return;
            }

            videos_checked += 1;
            state.send_log("INFO", &format!(
                "==== [{}/{}] 检查视频: {} ({}) ====",
                videos_checked, videos.len(), video.title, video.bvid
            ));

            // 如果配置了 only_bvid，跳过非指定视频
            if !config.reply.only_bvid.is_empty()
                && video.bvid != config.reply.only_bvid
            {
                state.send_log("DEBUG", &format!("跳过视频 {}: 不在 only_bvid 范围内", video.bvid));
                continue;
            }

            state.rate_limiter.wait();

            state.send_log("DEBUG", &format!("请求评论: {} 页数上限={}", video.bvid, config.bilibili.max_comment_pages));
            match comment_fetcher::get_video_comments(
                &http_client,
                &video.bvid,
                config.bilibili.max_comment_pages,
                config.reply.chained_reply_enabled,
                config.reply.max_reply_depth,
            )
            .await
            {
                Ok(comments) => {
                    let total = comments.len();
                    state.send_log("INFO", &format!("视频 {} 获取到 {} 条评论（含楼中楼）", video.bvid, total));
                    process_comments(&state, &http_client, &config, video, &comments).await;
                }
                Err(e) => {
                    state.send_log("ERROR", &format!("获取评论失败 [{}]: {}", video.bvid, e));
                    state.rate_limiter.record_failure();
                }
            }
        }

        // 等待下次检查
        let interval = config.bilibili.check_interval.max(1);
        state.send_stats(
            state.history.lock().await.total_replied(),
            state.rate_limiter.failure_count(),
        );
        state.send_log("INFO", &format!(
            "==== 第 {} 轮完成 等待 {} 秒 ====",
            round, interval
        ));

        for _ in 0..interval {
            if state.shutdown.load(Ordering::Relaxed) {
                return;
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    state.send_log("INFO", "机器人主循环已退出");
}

/// 处理单条视频的所有评论
async fn process_comments(
    state: &Arc<BotState>,
    client: &reqwest::Client,
    config: &RawConfig,
    video: &VideoInfo,
    comments: &[Comment],
) {
    // 预览模式：跳过所有B站API相关校验，仅测试AI回复生成
    let dry_run = {
        let cfg = state.config.read().await;
        cfg.reply.dry_run
    };
    if dry_run {
        state.send_log("INFO", "🔍 预览模式已开启 —— 仅生成AI回复，不会实际发表");
    }

    // 非预览模式才需要 CSRF 和 Cookie 校验
    let csrf = if !dry_run {
        let cm = state.cookie_manager.lock().await;
        let token = cm.get_csrf_from_cookie();
        let token = match token {
            Some(c) => c,
            None => {
                state.send_log("ERROR", "CSRF Token (bili_jct) 缺失，跳过评论处理。请确认 Cookie 包含 bili_jct 字段。");
                return;
            }
        };

        // 回复前复验 Cookie 有效性
        {
            let verify = cm.verify_cookie().await;
            if !verify.valid {
                state.send_log("ERROR", &format!("Cookie 无效，跳过评论处理: {}", verify.message));
                return;
            }
        }

        token
    } else {
        String::new() // dry_run 模式不需要
    };

    let mut processed_count = 0u32;
    let mut skipped_count = 0u32;
    let max_process = config.reply.max_process;
    let total_comments = comments.len();

    if total_comments == 0 {
        state.send_log("INFO", "视频无评论，跳过");
        return;
    }

    state.send_log("INFO", &format!(
        "开始处理视频 {} 的 {} 条评论（max_process={} dry_run={}）",
        video.bvid, total_comments, max_process, dry_run
    ));
    state.send_log("DEBUG", "进入评论循环...");

    for (i, comment) in comments.iter().enumerate() {
        state.send_log("DEBUG", &format!(
            "  [{}/{}] 遍历: user={} content_preview=\"{}\" depth={}",
            i+1, total_comments,
            comment.user,
            truncate_str(&comment.content, 30),
            comment.depth
        ));

        if state.shutdown.load(Ordering::Relaxed) || processed_count >= max_process {
            if processed_count >= max_process {
                state.send_log("INFO", "达到 max_process 上限，停止处理本视频");
            }
            break;
        }

        // 预览模式重新读取实时配置
        let is_dry_run = {
            let cfg = state.config.read().await;
            cfg.reply.dry_run
        };
        state.send_log("DEBUG", &format!("  is_dry_run={}", is_dry_run));

        let comment_idx = processed_count + skipped_count + 1;

        // 跳过已处理的评论
        {
            let history = state.history.lock().await;
            if history.is_processed(&comment.comment_id) {
                skipped_count += 1;
                state.send_log("INFO", &format!(
                    "[{}/{}] ⏭ 跳过已处理: {}",
                    comment_idx, total_comments, comment.user
                ));
                state.send_log("DEBUG", "  history 锁释放（continue）");
                continue;
            }
            state.send_log("DEBUG", "  comment 未处理，继续");
        }

        // 跳过本人评论（仅非预览模式）
        state.send_log("DEBUG", &format!("  本人检查: is_dry_run={} comment_uid=\"{}\" config_uid=\"{}\"", is_dry_run, comment.uid, config.bilibili.uid));
        if !is_dry_run && comment.uid == config.bilibili.uid {
            skipped_count += 1;
            state.send_log("INFO", &format!(
                "[{}/{}] ⏭ 跳过本人评论: {}", comment_idx, total_comments, comment.user
            ));
            continue;
        }

        state.send_log("INFO", &format!(
            "[{}/{}] 处理评论: {} → \"{}\"",
            comment_idx, total_comments,
            comment.user,
            truncate_str(&comment.content, 40)
        ));

        // 频率控制等待
        state.rate_limiter.wait();
        state.send_log("DEBUG", &format!("  频率等待完成, 连续失败={}", state.rate_limiter.failure_count()));

        // 收集上下文评论
        let context = collect_context(comments, comment, config.reply.context_comments_count);
        if !context.is_empty() {
            state.send_log("DEBUG", &format!("  附带 {} 条上下文评论", context.len()));
        }

        // 生成 AI 回复
        let ai_provider_name = match config.ai.provider {
            AiProvider::Deepseek => "DeepSeek",
            AiProvider::Ollama => "Ollama",
        };
        state.send_log("INFO", &format!("  => 调用 {} 生成回复...", ai_provider_name));
        let reply_text = match config.ai.provider {
            AiProvider::Deepseek => {
                deepseek::generate_reply(
                    client,
                    &config.deepseek,
                    &comment.content,
                    &context,
                    Some(&video.title),
                    Some(&video.desc),
                ).await
            }
            AiProvider::Ollama => {
                ollama::generate_reply(
                    client,
                    &config.ollama,
                    &comment.content,
                    &context,
                    Some(&video.title),
                    Some(&video.desc),
                ).await
            }
        };

        let reply_text = match reply_text {
            Ok(t) => {
                state.send_log("DEBUG", &format!("  AI 原始回复: \"{}\"", truncate_str(&t, 80)));
                t
            }
            Err(e) => {
                state.send_log("ERROR", &format!("AI回复生成失败: {}", e));
                state.rate_limiter.record_failure();
                continue;
            }
        };

        // 添加回复前缀
        let prefix = &config.reply.prefix;
        let full_reply = if prefix.is_empty() {
            reply_text
        } else {
            format!("{}{}", prefix, reply_text)
        };

        // 预览模式：仅日志输出生成的回复，不发表、不存历史
        if is_dry_run {
            state.send_log(
                "PREVIEW",
                &format!(
                    "[DRY RUN] {} 的回复: {}",
                    comment.user,
                    truncate_str(&full_reply, 100)
                ),
            );
            state.rate_limiter.record_success();
            processed_count += 1;
            continue;
        }

        // 发送回复
        state.rate_limiter.wait();
        let root_id = comment.root_id.as_deref().or(Some(&comment.comment_id));
        let parent_id = comment.parent_id.as_deref().or(Some(&comment.comment_id));

        state.send_log("DEBUG", &format!(
            "  发表回复: bvid={} root={} parent={} content_len={}",
            &video.bvid[..video.bvid.len().min(12)],
            root_id.unwrap_or("?"),
            parent_id.unwrap_or("?"),
            full_reply.len()
        ));

        match reply::reply_comment(
            client,
            &video.bvid,
            &comment.comment_id,
            &full_reply,
            &csrf,
            root_id,
            parent_id,
        ).await
        {
            Ok(None) => {
                state.send_log(
                    "INFO",
                    &format!("回复成功: {} → {}", comment.user, truncate_str(&full_reply, 50)),
                );

                // 保存到历史
                {
                    let history = state.history.lock().await;
                    history.add(
                        &comment.comment_id,
                        &video.bvid,
                        &video.title,
                        &comment.content,
                        &comment.user,
                        &comment.uid,
                        comment.ctime,
                        &full_reply,
                        comment.parent_id.as_deref(),
                        comment.root_id.as_deref(),
                        comment.depth,
                    );
                }
                state.rate_limiter.record_success();
                processed_count += 1;

                // 点赞评论（可选）
                if config.reply.like_enabled {
                    state.rate_limiter.wait();
                    let _ = reply::like_comment(client, &video.bvid, &comment.comment_id, &csrf).await;
                }

                // 点赞用户视频（可选）
                if config.reply.like_user_video_enabled {
                    if config.reply.like_user_video_only_followers
                        && !config.bilibili.uid.is_empty()
                    {
                        state.rate_limiter.wait();
                        if let Ok(is_follower) = reply::check_is_follower(
                            client,
                            &config.bilibili.uid,
                            &comment.uid,
                        ).await
                        {
                            if !is_follower {
                                continue;
                            }
                        }
                    }
                    if let Ok(Some((user_video_bvid, _))) =
                        reply::get_user_latest_video(client, &comment.uid).await
                    {
                        state.rate_limiter.wait();
                        let _ = reply::like_video(client, &user_video_bvid).await;
                    }
                }
            }
            Ok(Some(ref msg)) => {
                state.send_log("WARN", &format!("回复失败 [{}]: {}", comment.user, msg));
                state.rate_limiter.record_failure();
            }
            Err(e) => {
                state.send_log("ERROR", &format!("回复异常: {} - {}", comment.user, e));
                state.rate_limiter.record_failure();
            }
        }

        // 回复延迟
        if processed_count < max_process {
            tokio::time::sleep(tokio::time::Duration::from_secs(
                config.reply.reply_delay,
            ))
            .await;
        }
    }

    // 视频处理完毕，输出汇总日志
    if dry_run {
        state.send_log("INFO", &format!(
            "==== 视频 {} 预览完成: 处理 {} 条, 跳过 {} 条 ====",
            video.bvid, processed_count, skipped_count
        ));
    } else {
        state.send_log("INFO", &format!(
            "==== 视频 {} 处理完成: 回复 {} 条, 跳过 {} 条 ====",
            video.bvid, processed_count, skipped_count
        ));
    }
}

/// 收集评论的上下文评论
fn collect_context(
    all_comments: &[Comment],
    current: &Comment,
    max_count: u32,
) -> Vec<Comment> {
    if max_count == 0 {
        return Vec::new();
    }
    let current_id = &current.comment_id;
    all_comments
        .iter()
        .filter(|c| c.comment_id != *current_id)
        .take(max_count as usize)
        .cloned()
        .collect()
}

/// 安全截断字符串到 max_chars 个字符（UTF-8 边界安全，中文友好）
fn truncate_str(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}
