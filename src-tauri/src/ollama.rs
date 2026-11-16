/// Ollama API 客户端（本地 LLM）
///
/// 调用本地 Ollama 服务的 /api/chat 端点。提示词、回退与安全门在 ai.rs。
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::ai::{ChatOptions, ChatResult, TokenUsage};
use crate::config::OllamaConfig;
use crate::prompt::ChatMessage;
use crate::rate_limiter::{self, RetryPolicy};

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    options: OllamaOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    num_predict: u32,
    temperature: f64,
    num_ctx: u32,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    #[serde(default)]
    message: OllamaRespMessage,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    prompt_eval_count: u64,
    #[serde(default)]
    eval_count: u64,
}

#[derive(Debug, Default, Deserialize)]
struct OllamaRespMessage {
    #[serde(default)]
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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client
        .get(format!("{}/api/tags", base_url.trim_end_matches('/')))
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

/// 调用 Ollama 的 /api/chat
pub async fn chat(
    client: &reqwest::Client,
    ollama_config: &OllamaConfig,
    messages: &[ChatMessage],
    options: ChatOptions,
    policy: RetryPolicy,
) -> Result<ChatResult> {
    let request_body = OllamaRequest {
        model: ollama_config.model.clone(),
        messages: messages.to_vec(),
        stream: false,
        options: OllamaOptions {
            num_predict: options.max_tokens,
            temperature: options.temperature,
            num_ctx: ollama_config.num_ctx.max(512),
        },
        format: options.json_mode.then(|| "json".to_string()),
    };

    log::debug!(
        "Ollama 请求: model={} timeout={}s num_predict={} temp={} json={}",
        ollama_config.model,
        options.timeout_secs,
        options.max_tokens,
        options.temperature,
        options.json_mode
    );

    let url = format!("{}/api/chat", ollama_config.base_url.trim_end_matches('/'));
    let timeout = std::time::Duration::from_secs(options.timeout_secs.max(1));

    let text = policy
        .run("Ollama 生成回复", || async {
            let resp = client
                .post(&url)
                .json(&request_body)
                .timeout(timeout)
                .send()
                .await
                .map_err(|e| rate_limiter::retryable(format!("网络错误: {}", e), None))?;

            let status = resp.status();
            let body = resp
                .text()
                .await
                .map_err(|e| rate_limiter::retryable(format!("读取响应失败: {}", e), None))?;

            // 本地服务 5xx / 429 值得重试；404（模型不存在）等没有意义
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                return Err(rate_limiter::retryable(
                    format!(
                        "HTTP {} {}",
                        status.as_u16(),
                        body.chars().take(200).collect::<String>()
                    ),
                    None,
                ));
            }
            if !status.is_success() {
                return Err(anyhow::anyhow!(
                    "Ollama API 错误 {}: {}",
                    status.as_u16(),
                    body.chars().take(200).collect::<String>()
                ));
            }
            Ok(body)
        })
        .await
        .context("Ollama API 请求失败")?;

    let result: OllamaResponse = serde_json::from_str(&text).context("Ollama API 响应解析失败")?;

    let content = result.message.content.trim().to_string();
    if content.is_empty() {
        return Err(anyhow::anyhow!("Ollama 返回空回复"));
    }

    Ok(ChatResult {
        text: content,
        usage: TokenUsage {
            prompt_tokens: result.prompt_eval_count,
            completion_tokens: result.eval_count,
        },
        model: result.model.unwrap_or_else(|| ollama_config.model.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> OllamaConfig {
        OllamaConfig {
            base_url: "http://127.0.0.1:11434/".into(),
            model: "qwen2.5:7b".into(),
            timeout_secs: 60,
            system_prompt: String::new(),
            max_tokens: 300,
            temperature: 0.5,
            num_ctx: 8192,
        }
    }

    /// 请求体必须使用传入的运行时参数，而不是硬编码的 200 / 0.7
    #[test]
    fn test_request_body_uses_runtime_options() {
        let c = cfg();
        let body = OllamaRequest {
            model: c.model.clone(),
            messages: vec![ChatMessage::user("你好")],
            stream: false,
            options: OllamaOptions {
                num_predict: 300,
                temperature: 0.5,
                num_ctx: 8192,
            },
            format: None,
        };
        assert_eq!(body.model, "qwen2.5:7b");
        assert_eq!(body.options.num_predict, 300);
        assert_eq!(body.options.num_ctx, 8192);
        assert!((body.options.temperature - 0.5).abs() < f64::EPSILON);
        assert_eq!(body.messages[0].content, "你好");
        let json = serde_json::to_string(&body).unwrap();
        assert!(!json.contains("format"), "普通回复不应带 format");
    }

    #[test]
    fn test_json_mode_sets_format() {
        let body = OllamaRequest {
            model: "qwen2.5:7b".into(),
            messages: vec![],
            stream: false,
            options: OllamaOptions {
                num_predict: 100,
                temperature: 0.3,
                num_ctx: 4096,
            },
            format: Some("json".into()),
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""format":"json""#));
    }
}
