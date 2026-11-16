/// 统一 AI 调度层
///
/// 负责三件事：
/// 1. 在 DeepSeek / Ollama / OpenAI 兼容端点之间分发请求，并在主 provider
///    失败后按配置回退；
/// 2. 把安全门的 JSON 决策解析成「回复 / 跳过」，低置信度时追加一次复核；
/// 3. 汇总 token 用量，供历史库统计与每日预算判断。
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::comment_fetcher::Comment;
use crate::config::{AiFallback, AiGuardConfig, AiProvider, RawConfig};
use crate::deepseek;
use crate::ollama;
use crate::openai;
use crate::prompt::{self, ChatMessage};
use crate::rate_limiter::RetryPolicy;

/// 单次请求的通用参数
#[derive(Debug, Clone, Copy)]
pub struct ChatOptions {
    pub max_tokens: u32,
    pub temperature: f64,
    pub json_mode: bool,
    pub timeout_secs: u64,
}

/// token 用量（输入 + 输出）
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl TokenUsage {
    pub fn total(self) -> u64 {
        self.prompt_tokens.saturating_add(self.completion_tokens)
    }

    pub fn add(&mut self, other: TokenUsage) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
    }
}

/// provider 返回的原始结果
#[derive(Debug, Clone)]
pub struct ChatResult {
    pub text: String,
    pub usage: TokenUsage,
    pub model: String,
}

/// 最终动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiAction {
    Reply,
    Skip,
}

/// 一条评论的 AI 处理结果
#[derive(Debug, Clone)]
pub struct AiReplyResult {
    pub action: AiAction,
    pub text: String,
    pub reason: String,
    pub risk: String,
    pub confidence: f64,
    pub reviewed: bool,
    pub provider: AiProvider,
    pub model: String,
    pub usage: TokenUsage,
    pub fallback_used: bool,
}

/// 连接测试结果
#[derive(Debug, Clone, Serialize)]
pub struct AiTestResult {
    pub provider: String,
    pub model: String,
    pub latency_ms: u64,
    pub reply: String,
}

/// 模型返回的结构化决策
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DecisionDraft {
    #[serde(default)]
    action: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    risk: String,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    reply: String,
}

/// 当前 provider 实际使用的系统提示词。
///
/// Ollama / OpenAI 段留空时沿用 DeepSeek 的人设，避免「换了引擎人设就失效」。
pub fn system_prompt_for(config: &RawConfig, provider: AiProvider) -> String {
    match provider {
        AiProvider::Deepseek => config.deepseek.system_prompt.clone(),
        AiProvider::Ollama => {
            if config.ollama.system_prompt.trim().is_empty() {
                config.deepseek.system_prompt.clone()
            } else {
                config.ollama.system_prompt.clone()
            }
        }
        AiProvider::Openai => {
            if config.openai.system_prompt.trim().is_empty() {
                config.deepseek.system_prompt.clone()
            } else {
                config.openai.system_prompt.clone()
            }
        }
    }
}

fn options_for(config: &RawConfig, provider: AiProvider, json_mode: bool) -> ChatOptions {
    match provider {
        AiProvider::Deepseek => ChatOptions {
            max_tokens: config.deepseek.max_tokens,
            temperature: config.deepseek.temperature,
            json_mode,
            timeout_secs: config.ai.request_timeout_secs.max(1),
        },
        AiProvider::Ollama => ChatOptions {
            max_tokens: config.ollama.max_tokens,
            temperature: config.ollama.temperature,
            json_mode,
            timeout_secs: config.ollama.timeout_secs.max(1),
        },
        AiProvider::Openai => ChatOptions {
            max_tokens: config.openai.max_tokens,
            temperature: config.openai.temperature,
            json_mode: json_mode && config.openai.json_mode,
            timeout_secs: config.ai.request_timeout_secs.max(1),
        },
    }
}

async fn call_provider(
    client: &reqwest::Client,
    config: &RawConfig,
    provider: AiProvider,
    messages: &[ChatMessage],
    options: ChatOptions,
    policy: RetryPolicy,
) -> Result<ChatResult> {
    match provider {
        AiProvider::Deepseek => deepseek::chat(client, &config.deepseek, messages, options, policy)
            .await
            .context("DeepSeek 调用失败"),
        AiProvider::Ollama => ollama::chat(client, &config.ollama, messages, options, policy)
            .await
            .context("Ollama 调用失败"),
        AiProvider::Openai => openai::chat(client, &config.openai, messages, options, policy)
            .await
            .context("OpenAI 兼容端点调用失败"),
    }
}

fn fallback_provider(config: &RawConfig, primary: AiProvider) -> Option<AiProvider> {
    let candidate = match config.ai.fallback {
        AiFallback::None => None,
        other => other.as_provider(),
    }?;
    if candidate == primary || !config.ai_provider_configured(candidate) {
        None
    } else {
        Some(candidate)
    }
}

/// 先调主 provider，失败后按配置回退一次
async fn call_with_fallback(
    client: &reqwest::Client,
    config: &RawConfig,
    messages: &[ChatMessage],
    json_mode: bool,
    policy: RetryPolicy,
) -> Result<(ChatResult, AiProvider, bool)> {
    let primary = config.ai.provider;
    let options = options_for(config, primary, json_mode);
    match call_provider(client, config, primary, messages, options, policy).await {
        Ok(result) => Ok((result, primary, false)),
        Err(primary_err) => {
            let Some(backup) = fallback_provider(config, primary) else {
                return Err(primary_err);
            };
            log::warn!(
                "{} 调用失败，尝试回退到 {}: {}",
                primary.label(),
                backup.label(),
                primary_err
            );
            let options = options_for(config, backup, json_mode);
            let result = call_provider(client, config, backup, messages, options, policy)
                .await
                .with_context(|| format!("回退到 {} 也失败", backup.label()))?;
            Ok((result, backup, true))
        }
    }
}

fn parse_decision(raw: &str) -> Option<DecisionDraft> {
    let json = prompt::extract_json_object(raw)?;
    let mut decision: DecisionDraft = serde_json::from_str(&json).ok()?;
    decision.action = decision.action.trim().to_ascii_lowercase();
    decision.risk = decision.risk.trim().to_ascii_lowercase();
    decision.reply = decision.reply.trim().to_string();
    decision.reason = decision.reason.trim().to_string();
    decision.confidence = if decision.confidence.is_finite() {
        decision.confidence.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if decision.action != "reply" && decision.action != "skip" {
        decision.action = if decision.reply.is_empty() {
            "skip".into()
        } else {
            "reply".into()
        };
    }
    if decision.risk.is_empty() {
        decision.risk = "uncertain".into();
    }
    Some(decision)
}

fn plain_decision(raw: &str) -> DecisionDraft {
    DecisionDraft {
        action: "reply".into(),
        reason: String::new(),
        risk: "uncertain".into(),
        confidence: 0.5,
        reply: raw.trim().to_string(),
    }
}

fn can_skip(decision: &DecisionDraft, guard: &AiGuardConfig) -> bool {
    decision.action == "skip"
        && decision.confidence >= guard.min_confidence
        && prompt::skip_risk_keys(guard).contains(&decision.risk.as_str())
}

fn rejection_reason(decision: &DecisionDraft, guard: &AiGuardConfig) -> String {
    if !prompt::skip_risk_keys(guard).contains(&decision.risk.as_str()) {
        format!("风险类别 {} 未开启跳过", decision.risk)
    } else if decision.confidence < guard.min_confidence {
        format!(
            "置信度 {:.2} 低于阈值 {:.2}",
            decision.confidence, guard.min_confidence
        )
    } else {
        "模型判断为跳过".to_string()
    }
}

/// 生成一条评论的 AI 回复 / 跳过决策。
#[allow(clippy::too_many_arguments)]
pub async fn generate_reply(
    client: &reqwest::Client,
    config: &RawConfig,
    comment: &Comment,
    thread: &[Comment],
    recent_comments: &[Comment],
    video_title: Option<&str>,
    video_desc: Option<&str>,
    style_examples: &[String],
    avoid_replies: &[String],
    policy: RetryPolicy,
) -> Result<AiReplyResult> {
    let guard = &config.reply.ai_guard;
    let guard_on = prompt::guard_active(guard);
    let system_prompt = system_prompt_for(config, config.ai.provider);

    let input = prompt::PromptInput {
        system_prompt: &system_prompt,
        video_title,
        video_desc,
        thread,
        recent_comments,
        comment,
        style_examples,
        avoid_replies,
        guard,
    };
    let messages = prompt::build_reply_messages(&input);

    let (first, provider, fallback_used) =
        call_with_fallback(client, config, &messages, guard_on, policy).await?;
    let mut usage = first.usage;
    let mut model = first.model.clone();
    let mut provider = provider;
    let mut fallback_used = fallback_used;
    let mut reviewed = false;

    if !guard_on {
        let text = first.text.trim().to_string();
        if text.is_empty() {
            return Err(anyhow::anyhow!("AI 返回空回复"));
        }
        return Ok(AiReplyResult {
            action: AiAction::Reply,
            text,
            reason: String::new(),
            risk: "safe".into(),
            confidence: 1.0,
            reviewed,
            provider,
            model,
            usage,
            fallback_used,
        });
    }

    let mut decision = match parse_decision(&first.text) {
        Some(d) => d,
        None => {
            if prompt::extract_json_object(&first.text).is_some() {
                return Err(anyhow::anyhow!(
                    "安全门返回的 JSON 无法解析（可能被截断），为避免把 JSON 当回复发布，已放弃本次生成"
                ));
            }
            log::warn!("安全门返回内容不是合法 JSON，按普通回复处理");
            plain_decision(&first.text)
        }
    };

    let mut skip_allowed = can_skip(&decision, guard);
    let needs_review = guard.review_low_confidence
        && (decision.confidence < guard.min_confidence
            || (decision.action == "skip" && !skip_allowed)
            || (decision.action == "reply" && decision.reply.is_empty()));

    if needs_review {
        let first_json = serde_json::to_string(&decision).unwrap_or_default();
        let review_messages = prompt::build_review_messages(&input, &first_json);
        match call_with_fallback(client, config, &review_messages, true, policy).await {
            Ok((review, review_provider, review_fallback)) => {
                usage.add(review.usage);
                provider = review_provider;
                model = review.model;
                fallback_used = fallback_used || review_fallback;
                reviewed = true;
                if let Some(reviewed_decision) = parse_decision(&review.text) {
                    decision = reviewed_decision;
                    skip_allowed = can_skip(&decision, guard);
                } else {
                    log::warn!("复核返回内容不是合法 JSON，保留第一轮判断");
                }
            }
            Err(e) => log::warn!("低置信度复核失败，保留第一轮判断: {}", e),
        }
    }

    if skip_allowed {
        return Ok(AiReplyResult {
            action: AiAction::Skip,
            text: String::new(),
            reason: if decision.reason.is_empty() {
                "模型判断不适合回复".into()
            } else {
                decision.reason.clone()
            },
            risk: decision.risk.clone(),
            confidence: decision.confidence,
            reviewed,
            provider,
            model,
            usage,
            fallback_used,
        });
    }

    let mut text;
    if decision.action == "skip" {
        if decision.reply.is_empty() {
            // 用户关闭了该类别的跳过开关，但模型仍坚持跳过：用普通提示词再生成一次，
            // 避免这条评论永远拿不到回复（也避免下一轮反复消耗 token）。
            log::warn!(
                "模型坚持跳过但类别未开启（{}），改用普通回复提示词重新生成",
                rejection_reason(&decision, guard)
            );
            let plain_guard = AiGuardConfig {
                enabled: false,
                ..guard.clone()
            };
            let plain_input = prompt::PromptInput {
                system_prompt: &system_prompt,
                video_title,
                video_desc,
                thread,
                recent_comments,
                comment,
                style_examples,
                avoid_replies,
                guard: &plain_guard,
            };
            let plain_messages = prompt::build_reply_messages(&plain_input);
            let (plain, plain_provider, plain_fallback) =
                call_with_fallback(client, config, &plain_messages, false, policy).await?;
            usage.add(plain.usage);
            provider = plain_provider;
            model = plain.model;
            fallback_used = fallback_used || plain_fallback;
            text = plain.text.trim().to_string();
        } else {
            log::debug!(
                "模型要求跳过但未通过安全门（{}），改用其补充回复",
                rejection_reason(&decision, guard)
            );
            text = decision.reply.trim().to_string();
        }
    } else {
        text = decision.reply.trim().to_string();
    }

    if text.is_empty() {
        return Err(anyhow::anyhow!("AI 返回空回复"));
    }

    // 与近期回复过于相似时重写一次（只重写一次，避免无限循环）
    let threshold = config.reply.dedupe_threshold.clamp(0.0, 1.0);
    if threshold > 0.0 && !avoid_replies.is_empty() {
        let mut worst_score = 0.0f64;
        for old in avoid_replies {
            let score = prompt::similarity(&text, old);
            if score > worst_score {
                worst_score = score;
            }
        }
        if worst_score >= threshold {
            log::info!("AI 回复与近期回复相似度 {:.2}，尝试重写一次", worst_score);
            let mut avoid_more = Vec::with_capacity(avoid_replies.len() + 1);
            avoid_more.push(text.clone());
            avoid_more.extend_from_slice(avoid_replies);
            let retry_input = prompt::PromptInput {
                system_prompt: &system_prompt,
                video_title,
                video_desc,
                thread,
                recent_comments,
                comment,
                style_examples,
                avoid_replies: &avoid_more,
                guard,
            };
            let mut retry_messages = prompt::build_reply_messages(&retry_input);
            retry_messages.push(ChatMessage::user(
                "上一版回复与历史回复过于相似，请换一种表达重写。只输出最终结果。",
            ));
            match call_with_fallback(client, config, &retry_messages, guard_on, policy).await {
                Ok((retry, retry_provider, retry_fallback)) => {
                    usage.add(retry.usage);
                    provider = retry_provider;
                    model = retry.model;
                    fallback_used = fallback_used || retry_fallback;
                    let retry_decision = if guard_on {
                        match parse_decision(&retry.text) {
                            Some(d) => Some(d),
                            None if prompt::extract_json_object(&retry.text).is_some() => None,
                            None => Some(plain_decision(&retry.text)),
                        }
                    } else {
                        Some(plain_decision(&retry.text))
                    };
                    if let Some(retry_decision) = retry_decision {
                        if can_skip(&retry_decision, guard) {
                            return Ok(AiReplyResult {
                                action: AiAction::Skip,
                                text: String::new(),
                                reason: if retry_decision.reason.is_empty() {
                                    "重写时模型判断不适合回复".into()
                                } else {
                                    retry_decision.reason
                                },
                                risk: retry_decision.risk,
                                confidence: retry_decision.confidence,
                                reviewed: true,
                                provider,
                                model,
                                usage,
                                fallback_used,
                            });
                        }
                        let retry_text = retry_decision.reply.trim();
                        if !retry_text.is_empty() && prompt::similarity(retry_text, &text) < 1.0 {
                            text = retry_text.to_string();
                        }
                    } else {
                        log::warn!("相似度重写返回的 JSON 无法解析，沿用原回复");
                    }
                }
                Err(e) => log::warn!("相似度重写失败，沿用原回复: {}", e),
            }
        }
    }

    Ok(AiReplyResult {
        action: AiAction::Reply,
        text,
        reason: decision.reason,
        risk: decision.risk,
        confidence: decision.confidence,
        reviewed,
        provider,
        model,
        usage,
        fallback_used,
    })
}

/// 测试指定 provider 的连接（不启用回退，避免测不出主 provider 的真实问题）
pub async fn test_connection(
    client: &reqwest::Client,
    config: &RawConfig,
    provider: AiProvider,
    policy: RetryPolicy,
) -> Result<AiTestResult> {
    // 连接测试只回答 OK，不应被思考模式/推理强度拖慢或占满 16 个输出 token
    let mut probe_config = config.clone();
    probe_config.deepseek.thinking = false;
    probe_config.openai.reasoning_effort.clear();

    let messages = prompt::build_test_messages();
    let mut options = options_for(&probe_config, provider, false);
    options.max_tokens = 16;
    options.temperature = 0.0;

    let started = std::time::Instant::now();
    let result = call_provider(client, &probe_config, provider, &messages, options, policy).await?;
    Ok(AiTestResult {
        provider: provider.label().to_string(),
        model: result.model,
        latency_ms: started.elapsed().as_millis() as u64,
        reply: result.text.trim().chars().take(80).collect(),
    })
}

/// 拉取某个 provider 的模型列表
pub async fn list_models(
    client: &reqwest::Client,
    config: &RawConfig,
    provider: AiProvider,
) -> Result<Vec<String>> {
    match provider {
        AiProvider::Ollama => ollama::list_models(&config.ollama.base_url).await,
        AiProvider::Deepseek => {
            let key = crate::deepseek::normalize_api_key(&config.deepseek.api_key);
            openai::list_models(client, &config.deepseek.base_url, &key).await
        }
        AiProvider::Openai => {
            openai::list_models(client, &config.openai.base_url, &config.openai.api_key).await
        }
    }
}

/// 每日预算是否已经用尽
pub fn budget_exhausted(config: &RawConfig, used_tokens: u64) -> bool {
    config.ai.daily_token_budget > 0 && used_tokens >= config.ai.daily_token_budget
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> AiGuardConfig {
        AiGuardConfig::default()
    }

    #[test]
    fn test_parse_json_decision() {
        let d = parse_decision(
            "~~~json\n{\"action\":\"skip\",\"reason\":\"广告\",\"risk\":\"spam\",\"confidence\":0.9,\"reply\":\"\"}\n~~~",
        )
        .expect("应解析");
        assert_eq!(d.action, "skip");
        assert_eq!(d.risk, "spam");
        assert!(can_skip(&d, &guard()));
    }

    #[test]
    fn test_low_confidence_skip_is_not_allowed() {
        let d = parse_decision(
            "{\"action\":\"skip\",\"risk\":\"spam\",\"confidence\":0.3,\"reply\":\"\"}",
        )
        .expect("应解析");
        assert!(!can_skip(&d, &guard()));
        assert!(rejection_reason(&d, &guard()).contains("置信度"));
    }

    #[test]
    fn test_skip_category_disabled_is_not_allowed() {
        let mut g = guard();
        g.skip_spam = false;
        let d = parse_decision(
            "{\"action\":\"skip\",\"risk\":\"spam\",\"confidence\":0.9,\"reply\":\"\"}",
        )
        .expect("应解析");
        assert!(!can_skip(&d, &g));
    }

    #[test]
    fn test_plain_text_fallback_is_reply() {
        let d = plain_decision("感谢支持");
        assert_eq!(d.action, "reply");
        assert_eq!(d.reply, "感谢支持");
    }

    #[test]
    fn test_budget_exhausted() {
        let mut cfg = RawConfig::default();
        cfg.ai.daily_token_budget = 1000;
        assert!(!budget_exhausted(&cfg, 999));
        assert!(budget_exhausted(&cfg, 1000));
        cfg.ai.daily_token_budget = 0;
        assert!(!budget_exhausted(&cfg, 999999));
    }
}
