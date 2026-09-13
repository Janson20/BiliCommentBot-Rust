/// Ollama API 客户端（本地 LLM）
///
/// 调用本地 Ollama 服务的 /api/chat 端点。
///
/// 与 DeepSeek 路径保持一致：系统提示词、max_tokens、temperature 都取自配置，
/// 不再硬编码 —— 否则用户在「配置」里改的人设对本地模型完全不起作用。
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::comment_fetcher::Comment;
use crate::config::OllamaConfig;
use crate::deepseek::build_messages;
use crate::rate_limiter::{self, RetryPolicy};

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

/// 使用 Ollama 生成回复
///
/// `system_prompt` 为空时由 [`build_messages`] 回落到缺省提示词。
#[allow(clippy::too_many_arguments)]
pub async fn generate_reply(
    client: &reqwest::Client,
    ollama_config: &OllamaConfig,
    comment_text: &str,
    context: &[Comment],
    video_title: Option<&str>,
    video_desc: Option<&str>,
    system_prompt: &str,
    policy: RetryPolicy,
) -> Result<String> {
    let messages: Vec<OllamaMessage> = build_messages(
        system_prompt,
        comment_text,
        context,
        video_title,
        video_desc,
    )
    .into_iter()
    .map(|(role, content)| OllamaMessage { role, content })
    .collect();

    let request_body = OllamaRequest {
        model: ollama_config.model.clone(),
        messages,
        stream: false,
        options: OllamaOptions {
            num_predict: ollama_config.max_tokens,
            temperature: ollama_config.temperature,
        },
    };

    log::debug!(
        "Ollama 请求: model={} timeout={}s num_predict={} temp={}",
        ollama_config.model,
        ollama_config.timeout_secs,
        ollama_config.max_tokens,
        ollama_config.temperature
    );

    let url = format!("{}/api/chat", ollama_config.base_url.trim_end_matches('/'));
    let timeout = std::time::Duration::from_secs(ollama_config.timeout_secs.max(1));

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

    let result: OllamaResponse =
        serde_json::from_str(&text).context("Ollama API 响应解析失败")?;

    Ok(result.message.content.trim().to_string())
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
        }
    }

    /// 请求体必须使用配置里的数值，而不是硬编码的 200 / 0.7
    #[test]
    fn test_request_body_uses_config_values() {
        let c = cfg();
        let messages: Vec<OllamaMessage> =
            build_messages("自定义人设", "你好", &[], None, None)
                .into_iter()
                .map(|(role, content)| OllamaMessage { role, content })
                .collect();
        let body = OllamaRequest {
            model: c.model.clone(),
            messages,
            stream: false,
            options: OllamaOptions {
                num_predict: c.max_tokens,
                temperature: c.temperature,
            },
        };
        assert_eq!(body.model, "qwen2.5:7b");
        assert_eq!(body.options.num_predict, 300);
        assert!((body.options.temperature - 0.5).abs() < f64::EPSILON);
        assert_eq!(body.messages[0].content, "自定义人设");
    }

    #[test]
    fn test_default_ollama_config_values() {
        let d = OllamaConfig::default();
        assert_eq!(d.base_url, "http://127.0.0.1:11434");
        assert_eq!(d.model, "qwen2.5:7b");
        assert_eq!(d.max_tokens, 200);
        assert!((d.temperature - 0.7).abs() < f64::EPSILON);
        assert!(d.system_prompt.is_empty(), "默认留空以沿用 deepseek 的人设");
    }
}
