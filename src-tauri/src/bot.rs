/// 机器人主循环 — 评论处理编排器
///
/// 整合所有业务模块：频率控制、视频获取、评论获取、AI生成、回复、点赞
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, broadcast};

use crate::comment_fetcher::{self, Comment};
use crate::config::{
    AiProvider, KeywordFilterConfig, LengthFilterConfig, RawConfig, ReplyConfig, UserFilterConfig,
};
use crate::cookie::CookieManager;
use crate::deepseek;
use crate::history::HistoryManager;
use crate::http_client;
use crate::ollama;
use crate::paths;
use crate::rate_limiter::{RateLimiter, RetryPolicy};
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
    #[serde(rename = "video_list")]
    VideoList { count: usize, videos: Vec<VideoInfo> },
    #[serde(rename = "status")]
    Status { running: bool },
    /// 需要用户处理的问题（Cookie 失效、连续失败等），前端显示醒目横幅。
    ///
    /// 没有它的话，Cookie 过期后机器人会**静默**失败一整晚，只有翻日志才发现。
    #[serde(rename = "alert")]
    Alert {
        /// "error" | "warning"
        level: String,
        title: String,
        message: String,
    },
}

/// B站 评论的长度上限（按字符计），超出会被服务端拒绝，因此这里主动截断
pub const MAX_REPLY_CHARS: usize = 1000;

/// 连续失败达到该次数时提醒用户
const FAILURE_ALERT_THRESHOLD: u32 = 5;

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
    /// 手动触发下一轮检查的信号
    pub manual_trigger: AtomicBool,
    pub rate_limiter: RateLimiter,
    /// 历史数据库是否可用；不可用时拒绝启动机器人（避免重复回复）
    pub history_available: bool,
    /// 上次检查 Cookie 状态的时间，用于 `cookie_refresh_interval` 节流
    pub last_cookie_check: Mutex<Option<Instant>>,
}

impl BotState {
    pub fn send_log(&self, level: &str, msg: &str) {
        // 同时写进日志文件 —— 正式版没有控制台，文件是唯一的排查手段
        match level {
            "ERROR" => log::error!("{}", msg),
            "WARN" => log::warn!("{}", msg),
            "DEBUG" => log::debug!("{}", msg),
            _ => log::info!("{}", msg),
        }

        let entry = LogEntry {
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: level.to_string(),
            msg: msg.to_string(),
        };
        let _ = self.event_tx.send(BotEvent::Log(entry));
    }

    pub async fn send_stats(&self, total_replied: u64, consecutive_failures: u32) {
        let stats = BotStats {
            running: self.running.load(Ordering::Relaxed),
            total_replied,
            start_time: self.start_time.lock().await.clone(),
            last_check: self.last_check.lock().await.clone(),
            consecutive_failures,
        };
        let _ = self.event_tx.send(BotEvent::Stats(stats));
    }

    /// 推送一条需要用户注意的提醒（同时写日志），前端会显示横幅
    pub fn send_alert(&self, level: &str, title: &str, message: &str) {
        if level == "error" {
            log::error!("[提醒] {} — {}", title, message);
        } else {
            log::warn!("[提醒] {} — {}", title, message);
        }
        let _ = self.event_tx.send(BotEvent::Alert {
            level: level.to_string(),
            title: title.to_string(),
            message: message.to_string(),
        });
    }
}

// ════════════════════════════════════════════════════════════════
//  启动 / 停止
// ════════════════════════════════════════════════════════════════

/// 启动机器人。**命令与开机自启共用**这一入口，避免两处逻辑漂移。
pub fn try_start(state: &Arc<BotState>) -> Result<(), String> {
    if state.running.load(Ordering::Relaxed) {
        return Err("机器人已在运行中".into());
    }
    if !state.history_available {
        return Err(
            "历史数据库不可用，已拒绝启动：无法去重会导致重复回复。请检查数据目录是否可写，然后重启程序。"
                .into(),
        );
    }

    state.shutdown.store(false, Ordering::Relaxed);
    state.running.store(true, Ordering::Relaxed);
    start_bot(state.clone());
    let _ = state.event_tx.send(BotEvent::Status { running: true });
    state.send_log("INFO", "机器人已启动");
    Ok(())
}

/// 启动机器人后台任务（独立的 tokio task）
pub fn start_bot(state: Arc<BotState>) {
    tokio::spawn(async move {
        bot_main_loop(state).await;
    });
}

/// 单轮处理的全局预算（对标 Python：`max_process` 是**每轮所有视频合计**的上限，
/// 而不是每个视频各算一份 —— 否则有 N 个视频时一轮就可能发出 N 倍回复）。
struct RoundBudget {
    max_process: u32,
    processed: u32,
}

impl RoundBudget {
    fn new(max_process: u32) -> Self {
        Self {
            max_process,
            processed: 0,
        }
    }

    fn exhausted(&self) -> bool {
        self.processed >= self.max_process
    }

    fn consume(&mut self) {
        self.processed += 1;
    }
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
        .timeout(std::time::Duration::from_secs(30))
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

        // 检查配置热更新：排空所有待处理消息，只应用最新一条，
        // 避免短时间内多次保存导致 rate_limiter 用到过期配置
        let mut latest: Option<RawConfig> = None;
        while let Ok(cfg) = reload_rx.try_recv() {
            latest = Some(cfg);
        }
        if let Some(new_config) = latest {
            let rl = &new_config.rate_limit;
            state.rate_limiter.reconfigure(
                rl.min_request_interval,
                rl.max_retries,
                rl.retry_delay,
            );
            // 日志配置同样热更新（级别 / 文件 / 是否输出到控制台）
            crate::logger::apply(&new_config.logging);
            // bot_state.config 已由 save_config 即时写入，此处仅兜底保证一致
            *state.config.write().await = new_config.clone();
            state.send_log("INFO", &format!(
                "配置已热更新: AI={:?}, reply_enabled={}, check_interval={}s, max_process={}",
                new_config.ai.provider,
                new_config.reply.enabled,
                new_config.bilibili.check_interval,
                new_config.reply.max_process,
            ));
        }

        let config = state.config.read().await.clone();
        let policy = state.rate_limiter.retry_policy();

        state.send_log("DEBUG", &format!(
            "当前配置: AI={:?}, reply_enabled={}, max_process={}, dry_run={}, check_interval={}s",
            config.ai.provider, config.reply.enabled, config.reply.max_process, config.reply.dry_run, config.bilibili.check_interval
        ));

        *state.last_check.lock().await = Some(
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        );

        // ── 自动回复总开关 ──
        // 之前这个开关只被写进日志、从未真正生效：用户关掉「启用自动回复」
        // 之后机器人依然在公开发布回复。
        if !config.reply.enabled {
            state.send_log("INFO", "⛔ 「启用自动回复」已关闭，本轮跳过评论处理");
            state
                .send_stats(
                    state.history.lock().await.total_replied(),
                    state.rate_limiter.failure_count(),
                )
                .await;
            if wait_for_next_round(&state, &mut reload_rx, config.bilibili.check_interval, round)
                .await
            {
                break;
            }
            continue;
        }

        // ── Cookie 自动刷新（按 cookie_refresh_interval 节流）──
        // 未节流时每一轮都会去敲 passport 接口，25 秒的检查间隔意味着
        // 每天 3000+ 次无谓请求，白白增加风控暴露。
        if config.bilibili.auto_refresh_cookie {
            let interval_secs = config.bilibili.cookie_refresh_interval.max(1) * 60;
            let due = {
                let last = state.last_cookie_check.lock().await;
                match *last {
                    None => true,
                    Some(t) => t.elapsed() >= Duration::from_secs(interval_secs),
                }
            };

            if due {
                let mut cm = state.cookie_manager.lock().await;
                match cm.auto_refresh_if_needed().await {
                    Ok((refreshed, msg)) => {
                        if refreshed {
                            state.send_log("INFO", &format!("Cookie已自动刷新: {}", msg));
                        } else {
                            log::debug!("Cookie 状态检查: {}", msg);
                        }
                        let _ = cm.save_to_file(&paths::cookie_file());
                    }
                    Err(e) => log::warn!("Cookie 状态检查失败: {}", e),
                }
                drop(cm);
                *state.last_cookie_check.lock().await = Some(Instant::now());
            } else {
                log::debug!(
                    "Cookie 状态检查未到间隔（{} 分钟），跳过",
                    config.bilibili.cookie_refresh_interval
                );
            }
        }

        // ── 视频列表 ──
        // 指定了 only_bvid 时直接锁定该视频，不再拉取整个投稿列表
        // （对标 Python：only_bvid 是「目标」而不是「过滤器」，
        //  因此不需要配置 uid，也不会因为视频不在列表里而静默什么都不做）
        let videos = if !config.reply.only_bvid.trim().is_empty() {
            let bvid = config.reply.only_bvid.trim();
            if !crate::bvid::is_valid_bvid(bvid) {
                state.send_log("ERROR", &format!("only_bvid 不是合法的 BVID: {}", bvid));
                Vec::new()
            } else {
                state.send_log("INFO", &format!("已指定视频 {}，跳过投稿列表抓取", bvid));
                let info = VideoInfo {
                    bvid: bvid.to_string(),
                    title: format!("指定视频({})", bvid),
                    desc: String::new(),
                    play: 0,
                    comment: 0,
                };
                let _ = state.event_tx.send(BotEvent::VideoList {
                    count: 1,
                    videos: vec![info.clone()],
                });
                vec![info]
            }
        } else {
            match video_fetcher::get_video_list(
                &http_client,
                &config.bilibili.uid,
                config.bilibili.max_video_pages,
                &paths::resolve(&config.video_cache.cache_file),
                config.video_cache.expire_time,
                policy,
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
                    state.rate_limiter.record_failure();
                    if wait_for_next_round(
                        &state,
                        &mut reload_rx,
                        config.bilibili.check_interval,
                        round,
                    )
                    .await
                    {
                        break;
                    }
                    continue;
                }
            }
        };

        // 每轮共享一个预算（跨视频累计）
        let mut budget = RoundBudget::new(config.reply.max_process);
        let mut videos_checked = 0u32;

        for video in &videos {
            if state.shutdown.load(Ordering::Relaxed) {
                return;
            }
            if budget.exhausted() {
                state.send_log(
                    "INFO",
                    &format!(
                        "已达到 max_process={} 的每轮上限，停止处理剩余视频",
                        budget.max_process
                    ),
                );
                break;
            }

            videos_checked += 1;
            state.send_log("INFO", &format!(
                "==== [{}/{}] 检查视频: {} ({}) ====",
                videos_checked, videos.len(), video.title, video.bvid
            ));

            // 单个视频整体超时，防止任何环节卡死整个机器人
            let video_result = tokio::time::timeout(
                std::time::Duration::from_secs(120),
                async {
                    state.rate_limiter.wait().await;

                    state.send_log("DEBUG", &format!("请求评论: {} 页数上限={}", video.bvid, config.bilibili.max_comment_pages));
                    match comment_fetcher::get_video_comments(
                        &http_client,
                        &video.bvid,
                        config.bilibili.max_comment_pages,
                        config.reply.chained_reply_enabled,
                        config.reply.max_reply_depth,
                        policy,
                    )
                    .await
                    {
                        Ok(comments) => {
                            let total = comments.len();
                            state.send_log("INFO", &format!("视频 {} 获取到 {} 条评论（含楼中楼）", video.bvid, total));
                            process_comments(
                                &state,
                                &http_client,
                                &config,
                                video,
                                &comments,
                                policy,
                                &mut budget,
                            )
                            .await;
                        }
                        Err(e) => {
                            state.send_log("ERROR", &format!("获取评论失败 [{}]: {}", video.bvid, e));
                            state.rate_limiter.record_failure();
                        }
                    }
                },
            )
            .await;

            if video_result.is_err() {
                state.send_log("WARN", &format!("视频 {} 处理超时（120秒），跳过该视频", video.bvid));
            }
        }

        // 连续失败达到阈值时提醒用户（每轮至多一次，避免刷屏）
        let failures = state.rate_limiter.failure_count();
        if failures >= FAILURE_ALERT_THRESHOLD {
            state.send_alert(
                "warning",
                "连续请求失败",
                &format!(
                    "已连续失败 {} 次，可能触发了 B站 的频率限制或被风控。\
                     建议调大「最小请求间隔」，或稍后再试。",
                    failures
                ),
            );
        }

        // 等待下次检查
        state.send_stats(
            state.history.lock().await.total_replied(),
            failures,
        ).await;
        state.send_log("INFO", &format!(
            "==== 第 {} 轮完成（本轮回复 {} 条） 等待 {} 秒 ====",
            round,
            budget.processed,
            config.bilibili.check_interval.max(1)
        ));

        if wait_for_next_round(&state, &mut reload_rx, config.bilibili.check_interval, round).await
        {
            break;
        }
    }

    state.running.store(false, Ordering::Relaxed);
    let _ = state.event_tx.send(BotEvent::Status { running: false });
    state.send_log("INFO", "机器人主循环已退出");
}

/// 等待下一轮。返回 `true` 表示应当退出主循环。
async fn wait_for_next_round(
    state: &Arc<BotState>,
    reload_rx: &mut broadcast::Receiver<RawConfig>,
    interval: u64,
    _round: u64,
) -> bool {
    let interval = interval.max(1);
    for _ in 0..interval {
        if state.shutdown.load(Ordering::Relaxed) {
            return true;
        }
        if state.manual_trigger.swap(false, Ordering::Relaxed) {
            state.send_log("INFO", "收到手动触发，立即开始下一轮检查");
            return false;
        }
        // 配置更新时立即打断等待，使新配置（如 check_interval）即时生效；
        // 消息仍留在通道中，由下一轮顶部排空并应用
        if !reload_rx.is_empty() {
            state.send_log("INFO", "检测到配置更新，提前结束等待以应用新配置");
            return false;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    false
}

/// 处理单条视频的所有评论
#[allow(clippy::too_many_arguments)]
async fn process_comments(
    state: &Arc<BotState>,
    client: &reqwest::Client,
    config: &RawConfig,
    video: &VideoInfo,
    comments: &[Comment],
    policy: RetryPolicy,
    budget: &mut RoundBudget,
) {
    let total_comments = comments.len();
    if total_comments == 0 {
        state.send_log("INFO", "视频无评论，跳过");
        return;
    }

    // ── 取会话凭证（非预览模式才需要）──
    let dry_run_now = {
        let cfg = state.config.read().await;
        cfg.reply.dry_run
    };

    let (csrf, cookie_str) = if !dry_run_now {
        let cm = state.cookie_manager.lock().await;
        let token = match cm.get_csrf_from_cookie() {
            Some(c) => c,
            None => {
                state.send_log("ERROR", "CSRF Token (bili_jct) 缺失，跳过评论处理。请确认 Cookie 包含 bili_jct 字段。");
                state.send_alert(
                    "error",
                    "登录凭证缺少 bili_jct",
                    "无法发表回复。请在「登录」页面重新扫码，或手动填写完整的 Cookie。",
                );
                return;
            }
        };

        // 回复前复验 Cookie 有效性
        {
            let verify = cm.verify_cookie().await;
            if !verify.valid {
                state.send_log("ERROR", &format!("Cookie 无效，跳过评论处理: {}", verify.message));
                state.send_alert(
                    "error",
                    "B站登录已失效",
                    &format!(
                        "{}。机器人不会再发表任何回复，请在「登录」页面重新扫码登录。",
                        verify.message
                    ),
                );
                return;
            }
        }

        (token, cm.get_cookie_str())
    } else {
        state.send_log("INFO", "🔍 预览模式已开启 —— 仅生成AI回复，不会实际发表");
        (String::new(), String::new())
    };

    let mut processed_count = 0u32;
    let mut skipped_count = 0u32;

    state.send_log("INFO", &format!(
        "开始处理视频 {} 的 {} 条评论（每轮上限 {}，已用 {}）",
        video.bvid, total_comments, budget.max_process, budget.processed
    ));

    for (i, comment) in comments.iter().enumerate() {
        if state.shutdown.load(Ordering::Relaxed) || budget.exhausted() {
            if budget.exhausted() {
                state.send_log("INFO", "达到 max_process 上限，停止处理本视频");
            }
            break;
        }

        // 实时读取两个热开关，便于运行中即时生效
        let (is_dry_run, reply_enabled) = {
            let cfg = state.config.read().await;
            (cfg.reply.dry_run, cfg.reply.enabled)
        };
        if !reply_enabled {
            state.send_log("INFO", "「启用自动回复」已关闭，中止本视频剩余评论");
            break;
        }

        // 跳过已处理的评论
        {
            let history = state.history.lock().await;
            if history.is_processed(&comment.comment_id) {
                skipped_count += 1;
                state.send_log("DEBUG", &format!(
                    "[{}/{}] ⏭ 跳过已处理: {}",
                    i + 1, total_comments, comment.user
                ));
                continue;
            }
        }

        // 跳过本人评论（仅非预览模式）
        if !is_dry_run && !config.bilibili.uid.is_empty() && comment.uid == config.bilibili.uid {
            skipped_count += 1;
            state.send_log("INFO", &format!(
                "[{}/{}] ⏭ 跳过本人评论: {}", i + 1, total_comments, comment.user
            ));
            continue;
        }

        // 应用过滤器（关键词、长度、用户黑白名单）
        if let Some(reason) = check_filters(&config.reply, comment) {
            skipped_count += 1;
            state.send_log("DEBUG", &format!("跳过评论 {}: {}", comment.comment_id, reason));
            continue;
        }

        state.send_log("INFO", &format!(
            "[{}/{}] 处理评论: {} → \"{}\"",
            i + 1, total_comments,
            comment.user,
            truncate_str(&comment.content, 40)
        ));

        // 频率控制等待
        state.rate_limiter.wait().await;

        // 收集上下文评论（当前评论之前的 N 条 + 父评论）
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
                    policy,
                ).await
            }
            AiProvider::Ollama => {
                // 沿用 deepseek 段的系统提示词，除非 ollama 段自己配了一份，
                // 否则「换成本地模型后人设就失效」会让人莫名其妙。
                let system_prompt = if config.ollama.system_prompt.trim().is_empty() {
                    config.deepseek.system_prompt.as_str()
                } else {
                    config.ollama.system_prompt.as_str()
                };
                ollama::generate_reply(
                    client,
                    &config.ollama,
                    &comment.content,
                    &context,
                    Some(&video.title),
                    Some(&video.desc),
                    system_prompt,
                    policy,
                ).await
            }
        };

        let reply_text = match reply_text {
            Ok(t) => {
                state.send_log("DEBUG", &format!("  AI 原始回复: \"{}\"", truncate_str(&t, 80)));
                t
            }
            Err(e) => {
                // AI 失败**不计入** B站 请求的失败计数：否则一次 LLM 故障会把
                // 抓评论的间隔一路推到 10 倍，纯属误伤。
                state.send_log("ERROR", &format!("AI回复生成失败: {}", e));
                continue;
            }
        };

        // 跳过空回复
        if reply_text.trim().is_empty() {
            state.send_log("WARN", &format!("AI 回复为空，跳过评论 [{}]", comment.user));
            continue;
        }

        // 添加回复前缀
        let prefix = &config.reply.prefix;
        let full_reply = if prefix.is_empty() {
            reply_text
        } else {
            format!("{}{}", prefix, reply_text)
        };

        // 超过 B站 长度上限的回复会被服务端直接拒绝，这里主动截断
        let (full_reply, truncated) = clamp_reply(&full_reply, MAX_REPLY_CHARS);
        if truncated {
            state.send_log(
                "WARN",
                &format!("AI 回复超过 {} 字符，已截断后发表", MAX_REPLY_CHARS),
            );
        }

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
            budget.consume();
            continue;
        }

        // ── 发表回复 ──
        // root  = 楼中楼所在的根评论；parent = **被回复的那条评论**
        // 早期端口把 parent 也写成了根评论，导致回复挂到了根评论下而不是
        // 挂在被回复的子评论下（Python 用的是 comment.comment_id）。
        let root_id = comment.root_id.as_deref().unwrap_or(&comment.comment_id);
        let parent_id = comment.comment_id.as_str();

        state.rate_limiter.wait().await;
        match reply::reply_comment(
            client,
            &video.bvid,
            &comment.comment_id,
            &full_reply,
            &csrf,
            Some(root_id),
            Some(parent_id),
            &cookie_str,
            policy,
        ).await
        {
            Ok(None) => {
                state.send_log(
                    "INFO",
                    &format!("回复成功: {} → {}", comment.user, truncate_str(&full_reply, 50)),
                );

                // 保存到历史（去重依据）
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
                budget.consume();

                // 点赞评论（可选）
                if config.reply.like_enabled {
                    state.rate_limiter.wait().await;
                    match reply::like_comment(
                        client,
                        &video.bvid,
                        &comment.comment_id,
                        &csrf,
                        &cookie_str,
                        policy,
                    )
                    .await
                    {
                        Ok(true) => {}
                        Ok(false) => state.send_log("DEBUG", "点赞评论未生效（可能已点过赞）"),
                        Err(e) => state.send_log("WARN", &format!("点赞评论失败: {}", e)),
                    }
                }

                // 点赞用户最新视频（可选）
                if config.reply.like_user_video_enabled {
                    like_latest_video_of(
                        state,
                        client,
                        config,
                        comment,
                        &cookie_str,
                        policy,
                    )
                    .await;
                }

                // 回复延迟（对标 Python：仅在回复成功后等待）
                if config.reply.reply_delay > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(
                        config.reply.reply_delay,
                    ))
                    .await;
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
    }

    // 视频处理完毕，输出汇总日志
    if dry_run_now {
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

/// 点赞评论者的最新视频（含「仅粉丝」门槛）
///
/// 门槛语义对标 Python：询问「**评论者**是否关注了**我**」，
/// 因此调用 `check_is_follower(comment.uid, my_uid)`。
/// 未配置 uid 时 Python 会跳过点赞，这里保持一致 —— 宁可少点赞，
/// 也不要在无法判断时把赞点给陌生人。
async fn like_latest_video_of(
    state: &Arc<BotState>,
    client: &reqwest::Client,
    config: &RawConfig,
    comment: &Comment,
    cookie_str: &str,
    policy: RetryPolicy,
) {
    let my_uid = config.bilibili.uid.trim().to_string();

    if config.reply.like_user_video_only_followers {
        if my_uid.is_empty() {
            state.send_log(
                "WARN",
                "[点赞视频] 已开启「仅点赞粉丝视频」但未配置 UID，无法判断粉丝关系，跳过点赞视频",
            );
            return;
        }

        state.rate_limiter.wait().await;
        match reply::check_is_follower(client, &comment.uid, &my_uid, cookie_str, policy).await {
            Ok(true) => {}
            Ok(false) => {
                state.send_log(
                    "INFO",
                    &format!("[点赞视频] 用户 {} 未关注你，跳过点赞视频", comment.user),
                );
                return;
            }
            Err(e) => {
                state.send_log(
                    "WARN",
                    &format!("[点赞视频] 检查粉丝关系失败，跳过点赞视频: {}", e),
                );
                return;
            }
        }
    }

    match reply::get_user_latest_video(client, &comment.uid, cookie_str, policy).await {
        Ok(Some((user_video_bvid, _title))) => {
            state.rate_limiter.wait().await;
            match reply::like_video(client, &user_video_bvid, cookie_str, policy).await {
                Ok(true) => state.send_log(
                    "INFO",
                    &format!("[点赞视频] ✓ 已点赞 {} 的最新视频 {}", comment.user, user_video_bvid),
                ),
                Ok(false) => state.send_log(
                    "WARN",
                    &format!("[点赞视频] ✗ 点赞 {} 失败", user_video_bvid),
                ),
                Err(e) => state.send_log("WARN", &format!("[点赞视频] 点赞异常: {}", e)),
            }
        }
        Ok(None) => state.send_log(
            "DEBUG",
            &format!("[点赞视频] 用户 {} 没有视频或获取失败", comment.user),
        ),
        Err(e) => state.send_log("WARN", &format!("[点赞视频] 获取最新视频失败: {}", e)),
    }
}

/// 收集评论的上下文评论。
///
/// 对标 Python：取**当前评论之前**的 N 条（同一视频的对话顺序），
/// 再补上父评论。早期端口取的是整个列表的前 N 条，于是每条评论拿到的
/// "上下文"几乎一样，这个配置项等于白设。
fn collect_context(all_comments: &[Comment], current: &Comment, max_count: u32) -> Vec<Comment> {
    if max_count == 0 {
        return Vec::new();
    }

    let mut out: Vec<Comment> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    seen.insert(current.comment_id.as_str());

    // 1) 当前评论之前的 N 条
    if let Some(idx) = all_comments
        .iter()
        .position(|c| c.comment_id == current.comment_id)
    {
        let start = idx.saturating_sub(max_count as usize);
        for c in &all_comments[start..idx] {
            if seen.insert(c.comment_id.as_str()) {
                out.push(c.clone());
            }
        }
    }

    // 2) 父评论（楼中楼场景下最有价值的上下文）
    if let Some(pid) = current.parent_id.as_deref() {
        if let Some(parent) = all_comments.iter().find(|c| c.comment_id == pid) {
            if seen.insert(parent.comment_id.as_str()) {
                out.push(parent.clone());
            }
        }
    }

    out
}

/// 检查评论是否通过所有过滤器。返回 Some(原因) 表示未通过应跳过
/// 对标 Python 版 BiliCommentBot._check_filters
fn check_filters(reply_cfg: &ReplyConfig, comment: &Comment) -> Option<String> {
    // ── 长度过滤 ──
    if let Some(reason) = check_length_filter(&reply_cfg.length_filter, comment) {
        return Some(reason);
    }
    // ── 关键词过滤 ──
    if let Some(reason) = check_keyword_filter(&reply_cfg.keyword_filter, comment) {
        return Some(reason);
    }
    // ── 用户过滤 ──
    if let Some(reason) = check_user_filter(&reply_cfg.user_filter, comment) {
        return Some(reason);
    }
    None
}

/// 长度过滤
fn check_length_filter(lf: &LengthFilterConfig, comment: &Comment) -> Option<String> {
    if !lf.enabled {
        return None;
    }
    let content_len = comment.content.chars().count() as u32;
    if lf.min_length > 0 && content_len < lf.min_length {
        return Some(format!("评论长度 {} < {}", content_len, lf.min_length));
    }
    if lf.max_length > 0 && content_len > lf.max_length {
        return Some(format!("评论长度 {} > {}", content_len, lf.max_length));
    }
    None
}

/// 关键词过滤
fn check_keyword_filter(kf: &KeywordFilterConfig, comment: &Comment) -> Option<String> {
    if !kf.enabled {
        return None;
    }
    let bl_str = kf.blacklist.trim();
    let wl_str = kf.whitelist.trim();
    let match_case = kf.match_case;

    let content = if match_case {
        comment.content.clone()
    } else {
        comment.content.to_lowercase()
    };

    // 黑名单：命中任一即跳过
    if !bl_str.is_empty() {
        let keywords = split_keywords(bl_str, match_case);
        for kw in &keywords {
            if content.contains(kw) {
                return Some(format!("命中黑名单关键词: {}", kw));
            }
        }
    }

    // 白名单：根据 mode 决定匹配方式
    if !wl_str.is_empty() {
        let keywords = split_keywords(wl_str, match_case);
        let mode = kf.mode.trim().to_lowercase();
        if mode == "all" {
            if !keywords.iter().all(|kw| content.contains(kw)) {
                return Some("未包含所有白名单关键词".to_string());
            }
        } else if !keywords.iter().any(|kw| content.contains(kw)) {
            return Some("未包含任何白名单关键词".to_string());
        }
    }

    None
}

/// 拆分逗号分隔的关键词，并按 match_case 决定是否转小写
fn split_keywords(raw: &str, match_case: bool) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| if match_case { s } else { s.to_lowercase() })
        .collect()
}

/// 用户过滤
fn check_user_filter(uf: &UserFilterConfig, comment: &Comment) -> Option<String> {
    if !uf.enabled {
        return None;
    }
    let uid = &comment.uid;

    let bl_str = uf.blacklist.trim();
    let wl_str = uf.whitelist.trim();

    // 黑名单
    if !bl_str.is_empty() {
        let blacklist: Vec<&str> = bl_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if blacklist.iter().any(|u| u == uid) {
            return Some(format!("用户 {} 在黑名单中", uid));
        }
    }

    // 白名单
    if !wl_str.is_empty() {
        let whitelist: Vec<&str> = wl_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if !whitelist.iter().any(|u| u == uid) {
            return Some(format!("用户 {} 不在白名单中", uid));
        }
    }

    None
}

/// 安全截断字符串到 max_chars 个字符（UTF-8 边界安全，中文友好）
fn truncate_str(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// 把回复截断到 B站 允许的最大长度。
/// 返回 (截断后的文本, 是否发生了截断)。
fn clamp_reply(text: &str, max_chars: usize) -> (String, bool) {
    let len = text.chars().count();
    if len <= max_chars {
        (text.to_string(), false)
    } else {
        (text.chars().take(max_chars).collect(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmt(id: &str, content: &str, parent: Option<&str>, depth: u32) -> Comment {
        Comment {
            comment_id: id.into(),
            content: content.into(),
            user: format!("user-{}", id),
            uid: "1".into(),
            ctime: 0,
            parent_id: parent.map(|p| p.to_string()),
            root_id: parent.map(|p| p.to_string()),
            depth,
            children: vec![],
        }
    }

    #[test]
    fn test_collect_context_zero_disabled() {
        let all = vec![cmt("1", "a", None, 0)];
        assert!(collect_context(&all, &all[0], 0).is_empty(), "0 表示不附带上下文");
    }

    #[test]
    fn test_collect_context_takes_preceding_comments() {
        let all = vec![
            cmt("1", "第一条", None, 0),
            cmt("2", "第二条", None, 0),
            cmt("3", "当前评论", None, 0),
        ];
        let ctx = collect_context(&all, &all[2], 2);
        let ids: Vec<&str> = ctx.iter().map(|c| c.comment_id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2"], "应取当前评论之前的 2 条");
        assert!(
            !ctx.iter().any(|c| c.comment_id == "3"),
            "上下文不能包含当前评论自身"
        );
    }

    #[test]
    fn test_collect_context_limits_to_max() {
        let all = vec![
            cmt("1", "a", None, 0),
            cmt("2", "b", None, 0),
            cmt("3", "c", None, 0),
            cmt("4", "current", None, 0),
        ];
        let ctx = collect_context(&all, &all[3], 2);
        assert_eq!(ctx.len(), 2);
        let ids: Vec<&str> = ctx.iter().map(|c| c.comment_id.as_str()).collect();
        assert_eq!(ids, vec!["2", "3"], "应取最近的 2 条");
    }

    #[test]
    fn test_collect_context_includes_parent_for_nested_reply() {
        let all = vec![
            cmt("1", "主评论", None, 0),
            cmt("2", "子评论A", Some("1"), 1),
            cmt("3", "子评论B(当前)", Some("1"), 1),
        ];
        let ctx = collect_context(&all, &all[2], 1);
        let ids: Vec<&str> = ctx.iter().map(|c| c.comment_id.as_str()).collect();
        assert!(ids.contains(&"1"), "楼中楼必须带上父评论: {:?}", ids);
        assert!(ids.contains(&"2"), "也应带上前一条: {:?}", ids);
        assert!(!ids.contains(&"3"), "不含自身");
    }

    #[test]
    fn test_collect_context_no_duplicate_ids() {
        // 父评论同时也是"前一条"时不应重复
        let all = vec![cmt("1", "主评论", None, 0), cmt("2", "子评论", Some("1"), 1)];
        let ctx = collect_context(&all, &all[1], 5);
        let mut ids: Vec<&str> = ctx.iter().map(|c| c.comment_id.as_str()).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "上下文不应出现重复评论");
    }

    #[test]
    fn test_round_budget_is_global() {
        let mut b = RoundBudget::new(2);
        assert!(!b.exhausted());
        b.consume();
        assert!(!b.exhausted());
        b.consume();
        assert!(b.exhausted(), "达到上限后应停止处理");
    }

    #[test]
    fn test_round_budget_zero_stops_immediately() {
        assert!(RoundBudget::new(0).exhausted());
    }

    #[test]
    fn test_truncate_str_is_char_safe() {
        assert_eq!(truncate_str("你好世界", 2), "你好");
        assert_eq!(truncate_str("abc", 10), "abc");
    }

    #[test]
    fn test_clamp_reply_leaves_short_text_alone() {
        let (out, truncated) = clamp_reply("很短", MAX_REPLY_CHARS);
        assert_eq!(out, "很短");
        assert!(!truncated);
    }

    #[test]
    fn test_clamp_reply_truncates_long_text_at_char_boundary() {
        // 用中文构造超长文本：按字符截断，不能把多字节字符切成半个
        let long: String = "回".repeat(MAX_REPLY_CHARS + 500);
        let (out, truncated) = clamp_reply(&long, MAX_REPLY_CHARS);
        assert!(truncated, "应报告发生了截断");
        assert_eq!(out.chars().count(), MAX_REPLY_CHARS);
        // 结果仍是合法 UTF-8 且内容完整
        assert!(out.chars().all(|c| c == '回'));
    }

    #[test]
    fn test_clamp_reply_exact_boundary_is_not_truncated() {
        let exact: String = "a".repeat(MAX_REPLY_CHARS);
        let (_, truncated) = clamp_reply(&exact, MAX_REPLY_CHARS);
        assert!(!truncated, "正好等于上限不应算截断");
    }

    fn reply_cfg() -> ReplyConfig {
        ReplyConfig::default()
    }

    #[test]
    fn test_filters_disabled_by_default_pass() {
        let cfg = reply_cfg();
        assert!(check_filters(&cfg, &cmt("1", "随便一条", None, 0)).is_none());
    }

    #[test]
    fn test_keyword_blacklist_blocks() {
        let mut cfg = reply_cfg();
        cfg.keyword_filter.enabled = true;
        cfg.keyword_filter.blacklist = "广告, 加群".into();
        let hit = check_filters(&cfg, &cmt("1", "快来加群领福利", None, 0));
        assert!(hit.is_some());
        assert!(hit.unwrap().contains("加群"));
        assert!(check_filters(&cfg, &cmt("2", "视频很棒", None, 0)).is_none());
    }

    #[test]
    fn test_keyword_whitelist_any_and_all() {
        let mut cfg = reply_cfg();
        cfg.keyword_filter.enabled = true;
        cfg.keyword_filter.whitelist = "好评, 支持".into();

        cfg.keyword_filter.mode = "any".into();
        assert!(check_filters(&cfg, &cmt("1", "支持一下", None, 0)).is_none());
        assert!(check_filters(&cfg, &cmt("2", "路过", None, 0)).is_some());

        cfg.keyword_filter.mode = "all".into();
        assert!(check_filters(&cfg, &cmt("3", "支持", None, 0)).is_some());
        assert!(check_filters(&cfg, &cmt("4", "好评，支持", None, 0)).is_none());
    }

    #[test]
    fn test_keyword_match_case() {
        let mut cfg = reply_cfg();
        cfg.keyword_filter.enabled = true;
        cfg.keyword_filter.blacklist = "SPAM".into();

        cfg.keyword_filter.match_case = false;
        assert!(check_filters(&cfg, &cmt("1", "this is spam", None, 0)).is_some());

        cfg.keyword_filter.match_case = true;
        assert!(check_filters(&cfg, &cmt("2", "this is spam", None, 0)).is_none());
        assert!(check_filters(&cfg, &cmt("3", "this is SPAM", None, 0)).is_some());
    }

    #[test]
    fn test_length_filter_counts_chars_not_bytes() {
        let mut cfg = reply_cfg();
        cfg.length_filter.enabled = true;
        cfg.length_filter.min_length = 3;

        // 2 个汉字 = 6 字节，但字符数是 2，应被拦下
        assert!(check_filters(&cfg, &cmt("1", "你好", None, 0)).is_some());
        assert!(check_filters(&cfg, &cmt("2", "你好呀", None, 0)).is_none());
    }

    #[test]
    fn test_user_filter_whitelist_and_blacklist() {
        let mut cfg = reply_cfg();
        cfg.user_filter.enabled = true;
        cfg.user_filter.blacklist = "100, 200".into();

        let mut c = cmt("1", "hi", None, 0);
        c.uid = "100".into();
        assert!(check_filters(&cfg, &c).is_some());

        c.uid = "300".into();
        assert!(check_filters(&cfg, &c).is_none());

        cfg.user_filter.whitelist = "300, 400".into();
        assert!(check_filters(&cfg, &c).is_none());

        c.uid = "500".into();
        assert!(check_filters(&cfg, &c).is_some());
    }
}
