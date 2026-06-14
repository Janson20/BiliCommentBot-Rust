/// Ollama API 客户端（本地 LLM）
///
/// 调用本地 Ollama 服务的 /api/chat 端点
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::comment_fetcher::Comment;
use crate::config::OllamaConfig;

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    num_predict: u32,
    temperature: f64,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaRespMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaRespMessage {
    content: String,
}

/// 检测 Ollama 服务是否可用
pub async fn check_availability(base_url: &str) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    match client.get(base_url).send().await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// 获取 Ollama 可用模型列表
pub async fn list_models(base_url: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/tags", base_url))
        .send()
        .await
        .context("Ollama 获取模型列表失败")?;

    let json: serde_json::Value = resp.json().await?;
    let models = json["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// 使用 Ollama 生成回复
pub async fn generate_reply(
    client: &reqwest::Client,
    ollama_config: &OllamaConfig,
    comment_text: &str,
    context: &[Comment],
    video_title: Option<&str>,
    video_desc: Option<&str>,
) -> Result<String> {
    let system_prompt = "你是一个友善的B站UP主，请对评论做出自然、友好的回复。回复要简洁明了，控制在100字以内。";

    let mut messages = vec![OllamaMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
    }];

    // 构建上下文
    let mut ctx = String::new();
    if let Some(t) = video_title {
        ctx.push_str(&format!("视频标题：{}\n", t));
    }
    if let Some(d) = video_desc {
        if !d.is_empty() {
            ctx.push_str(&format!("视频简介：{}\n", d));
        }
    }
    if !context.is_empty() {
        ctx.push_str("前面的评论上下文：\n");
        for (i, c) in context.iter().enumerate() {
            ctx.push_str(&format!("{}. {}: {}\n", i + 1, c.user, c.content));
        }
    }
    if !ctx.is_empty() {
        messages.push(OllamaMessage {
            role: "user".to_string(),
            content: ctx,
        });
    }

    messages.push(OllamaMessage {
        role: "user".to_string(),
        content: comment_text.to_string(),
    });

    let request_body = OllamaRequest {
        model: ollama_config.model.clone(),
        messages,
        stream: false,
        options: OllamaOptions {
            num_predict: 200,
            temperature: 0.7,
        },
    };

    log::debug!("Ollama 请求: model={} timeout={}s", ollama_config.model, ollama_config.timeout_secs);

    let resp = client
        .post(format!("{}/api/chat", ollama_config.base_url))
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(ollama_config.timeout_secs))
        .send()
        .await
        .context("Ollama API 请求失败")?;

    let text = resp.text().await.context("Ollama API 响应读取失败")?;
    let result: OllamaResponse =
        serde_json::from_str(&text).context("Ollama API 响应解析失败")?;

    Ok(result.message.content.trim().to_string())
}
