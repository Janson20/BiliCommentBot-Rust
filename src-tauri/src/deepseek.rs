/// DeepSeek API 客户端
///
/// 对标 Python 版 generate_reply（DeepSeek 部分）
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::comment_fetcher::Comment;
use crate::config::DeepseekConfig;

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

/// 使用 DeepSeek API 生成回复
pub async fn generate_reply(
    client: &reqwest::Client,
    api_config: &DeepseekConfig,
    comment_text: &str,
    context: &[Comment],
    video_title: Option<&str>,
    video_desc: Option<&str>,
) -> Result<String> {
    let api_key = normalize_api_key(&api_config.api_key);
    if api_key.is_empty() {
        return Err(anyhow::anyhow!(
            "DeepSeek API Key 未设置，请在配置页面填写 API Key（以 sk- 开头）"
        ));
    }

    let mut messages = Vec::new();

    // System prompt
    let system_prompt = if api_config.system_prompt.is_empty() {
        "你是一个友善的B站UP主，请对评论做出自然、友好的回复。回复要简洁明了，控制在100字以内。".to_string()
    } else {
        api_config.system_prompt.clone()
    };
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
    });

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
            ctx_text.push_str("前面的评论上下文（已回复的历史评论，仅供参考）：\n");
            for (i, c) in context.iter().enumerate() {
                ctx_text.push_str(&format!("{}. {}: {}\n", i + 1, c.user, c.content));
            }
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: ctx_text,
        });
    }

    // 当前评论
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: comment_text.to_string(),
    });

    let request_body = ChatRequest {
        model: api_config.model.clone(),
        messages,
        max_tokens: api_config.max_tokens,
        temperature: api_config.temperature,
    };

    let resp = client
        .post(format!("{}/chat/completions", api_config.base_url))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(30))
        .json(&request_body)
        .send()
        .await
        .context("DeepSeek API 请求失败")?;

    let status = resp.status();
    let text = resp.text().await.context("DeepSeek API 响应读取失败")?;

    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "DeepSeek API 错误 {}: {}",
            status.as_u16(),
            &text[..text.len().min(200)]
        ));
    }

    let result: ChatResponse =
        serde_json::from_str(&text).context("DeepSeek API 响应解析失败")?;

    result
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| anyhow::anyhow!("DeepSeek 返回空回复"))
}
