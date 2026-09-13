/// DeepSeek API 客户端
///
/// 对标 Python 版 generate_reply（DeepSeek 部分），包含**失败重试**：
/// Python 会按 `rate_limit.max_retries` 做指数退避重试，早期 Rust 端口只请求一次，
/// 一次瞬时 5xx / 网络抖动就等于永久丢掉一条回复。
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::comment_fetcher::Comment;
use crate::config::DeepseekConfig;
use crate::rate_limiter::{self, RetryPolicy};

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f64,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMsgContent,
}

#[derive(Debug, Deserialize)]
struct ChatMsgContent {
    content: String,
}

/// 缺省系统提示词（配置留空时使用）
pub const DEFAULT_SYSTEM_PROMPT: &str =
    "你是一个友善的B站UP主，请对评论做出自然、友好的回复。回复要简洁明了，控制在100字以内。";

/// 规范化 API Key：去除首尾空白，若缺少 sk- 前缀则自动补全
fn normalize_api_key(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("sk-") {
        trimmed.to_string()
    } else {
        format!("sk-{}", trimmed)
    }
}

/// 构造请求消息体（DeepSeek 与 Ollama 共用同一套提示词结构）
pub fn build_messages(
    system_prompt: &str,
    comment_text: &str,
    context: &[Comment],
    video_title: Option<&str>,
    video_desc: Option<&str>,
) -> Vec<(String, String)> {
    let system = if system_prompt.trim().is_empty() {
        DEFAULT_SYSTEM_PROMPT.to_string()
    } else {
        system_prompt.to_string()
    };

    let mut messages = vec![("system".to_string(), system)];

    // 视频上下文
    let mut video_context = String::new();
    if let Some(t) = video_title {
        video_context.push_str(&format!("视频标题：{}\n", t));
    }
    if let Some(d) = video_desc {
        if !d.is_empty() {
            video_context.push_str(&format!("视频简介：{}\n", d));
        }
    }

    // 评论上下文
    if !context.is_empty() || !video_context.is_empty() {
        let mut ctx_text = video_context;
        if !context.is_empty() {
            ctx_text.push_str("前面的评论上下文（已回复的历史评论，仅供参考，请勿回复这些历史评论）：\n");
            for (i, c) in context.iter().enumerate() {
                ctx_text.push_str(&format!("{}. {}: {}\n", i + 1, c.user, c.content));
            }
        }
        messages.push(("user".to_string(), ctx_text.trim().to_string()));
    }

    messages.push(("user".to_string(), comment_text.to_string()));
    messages
}

/// 使用 DeepSeek API 生成回复
pub async fn generate_reply(
    client: &reqwest::Client,
    api_config: &DeepseekConfig,
    comment_text: &str,
    context: &[Comment],
    video_title: Option<&str>,
    video_desc: Option<&str>,
    policy: RetryPolicy,
) -> Result<String> {
    let api_key = normalize_api_key(&api_config.api_key);
    if api_key.is_empty() {
        return Err(anyhow::anyhow!(
            "DeepSeek API Key 未设置，请在配置页面填写 API Key（以 sk- 开头）"
        ));
    }

    let messages = build_messages(
        &api_config.system_prompt,
        comment_text,
        context,
        video_title,
        video_desc,
    )
    .into_iter()
    .map(|(role, content)| ChatMessage { role, content })
    .collect::<Vec<_>>();

    let request_body = ChatRequest {
        model: api_config.model.clone(),
        messages,
        max_tokens: api_config.max_tokens,
        temperature: api_config.temperature,
    };

    log::debug!(
        "DeepSeek 请求: model={} max_tokens={} temp={} msg_count={}",
        api_config.model,
        api_config.max_tokens,
        api_config.temperature,
        request_body.messages.len()
    );

    let url = format!("{}/chat/completions", api_config.base_url.trim_end_matches('/'));
    let auth = format!("Bearer {}", api_key);

    let text = policy
        .run("DeepSeek 生成回复", || async {
            let resp = client
                .post(&url)
                .header("Authorization", auth.clone())
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(30))
                .json(&request_body)
                .send()
                .await
                .map_err(|e| rate_limiter::retryable(format!("网络错误: {}", e), None))?;

            let status = resp.status();
            let retry_after = rate_limiter::parse_retry_after(resp.headers());
            let body = resp
                .text()
                .await
                .map_err(|e| rate_limiter::retryable(format!("读取响应失败: {}", e), None))?;

            // 限流与服务端错误值得重试；4xx（Key 无效 / 余额不足）重试没有意义
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                return Err(rate_limiter::retryable(
                    format!(
                        "HTTP {} {}",
                        status.as_u16(),
                        body.chars().take(200).collect::<String>()
                    ),
                    retry_after,
                ));
            }

            if !status.is_success() {
                return Err(anyhow::anyhow!(
                    "DeepSeek API 错误 {}: {}",
                    status.as_u16(),
                    body.chars().take(200).collect::<String>()
                ));
            }

            Ok(body)
        })
        .await
        .context("DeepSeek API 请求失败")?;

    let result: ChatResponse =
        serde_json::from_str(&text).context("DeepSeek API 响应解析失败")?;

    result
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| anyhow::anyhow!("DeepSeek 返回空回复"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_api_key() {
        assert_eq!(normalize_api_key("sk-abc"), "sk-abc");
        assert_eq!(normalize_api_key("  sk-abc "), "sk-abc");
        assert_eq!(normalize_api_key("abc123"), "sk-abc123");
        assert_eq!(normalize_api_key(""), "");
        assert_eq!(normalize_api_key("   "), "");
    }

    #[test]
    fn test_build_messages_uses_configured_system_prompt() {
        let msgs = build_messages("你是一只猫", "你好", &[], None, None);
        assert_eq!(msgs[0].0, "system");
        assert_eq!(msgs[0].1, "你是一只猫");
        assert_eq!(msgs.last().unwrap().1, "你好");
    }

    #[test]
    fn test_build_messages_falls_back_to_default_prompt() {
        let msgs = build_messages("   ", "你好", &[], None, None);
        assert_eq!(msgs[0].1, DEFAULT_SYSTEM_PROMPT);
    }

    #[test]
    fn test_build_messages_includes_video_and_context() {
        let ctx = vec![Comment {
            comment_id: "1".into(),
            content: "旧评论".into(),
            user: "小明".into(),
            uid: "9".into(),
            ctime: 0,
            parent_id: None,
            root_id: None,
            depth: 0,
            children: vec![],
        }];
        let msgs = build_messages("p", "新评论", &ctx, Some("标题"), Some("简介"));
        assert_eq!(msgs.len(), 3, "system + 上下文 + 当前评论");
        assert!(msgs[1].1.contains("标题"));
        assert!(msgs[1].1.contains("简介"));
        assert!(msgs[1].1.contains("小明: 旧评论"));
    }

    /// 真实 API 测试——需要有网络和有效 Key
    /// 运行时设置环境变量 DEEPSEEK_API_KEY=sk-xxx
    /// cargo test deepseek_generate -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_generate_reply_live() {
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .expect("请设置环境变量 DEEPSEEK_API_KEY=sk-xxx");
        let client = reqwest::Client::new();
        let cfg = DeepseekConfig {
            api_key,
            ..Default::default()
        };

        let result = generate_reply(
            &client,
            &cfg,
            "这个户型公摊好大啊，120平实际才90出头，还不如我家的老破小",
            &[],
            Some("看房日记：120平三房两卫实地测评"),
            Some("今天带大家看一套120平米的三房两卫，实地测量套内面积只有90出头..."),
            RetryPolicy::default(),
        )
        .await;

        match &result {
            Ok(reply) => println!("\n=== AI 回复 ===\n{}\n", reply),
            Err(e) => eprintln!("\n=== 失败 ===\n{}\n", e),
        }
        assert!(result.is_ok(), "DeepSeek API 调用失败");
        assert!(!result.unwrap().is_empty(), "回复不应为空");
    }
}
