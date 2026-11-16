/// DeepSeek API 客户端
///
/// 只负责 DeepSeek 的 HTTP 细节；提示词构建、回退与安全门在 ai.rs，
/// 请求重试沿用 rate_limiter::RetryPolicy。
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::ai::{ChatOptions, ChatResult, TokenUsage};
use crate::config::DeepseekConfig;
use crate::prompt::ChatMessage;
use crate::rate_limiter::{self, RetryPolicy};

/// 思考模式开关。
///
/// 官方参数（OpenAI 格式）为 {"thinking": {"type": "enabled" | "disabled"}}，
/// 直接作为请求体的顶层字段发送。线上模型默认是 enabled，
/// 所以这里总是显式发送，避免依赖服务端的默认值。
#[derive(Debug, Serialize)]
struct ThinkingToggle {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Serialize)]
struct JsonMode {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f64,
    thinking: ThinkingToggle,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<JsonMode>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<UsageRaw>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMsgContent,
}

#[derive(Debug, Deserialize)]
struct ChatMsgContent {
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct UsageRaw {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

/// 规范化 API Key：去除首尾空白，若缺少 sk- 前缀则自动补全
pub fn normalize_api_key(raw: &str) -> String {
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

fn build_request(
    api_config: &DeepseekConfig,
    messages: &[ChatMessage],
    options: ChatOptions,
) -> ChatRequest {
    ChatRequest {
        model: api_config.model.clone(),
        messages: messages.to_vec(),
        max_tokens: options.max_tokens,
        temperature: options.temperature,
        thinking: ThinkingToggle {
            kind: if api_config.thinking {
                "enabled"
            } else {
                "disabled"
            },
        },
        response_format: options.json_mode.then_some(JsonMode {
            kind: "json_object",
        }),
    }
}

/// 调用 DeepSeek 的 /chat/completions
pub async fn chat(
    client: &reqwest::Client,
    api_config: &DeepseekConfig,
    messages: &[ChatMessage],
    options: ChatOptions,
    policy: RetryPolicy,
) -> Result<ChatResult> {
    let api_key = normalize_api_key(&api_config.api_key);
    if api_key.is_empty() {
        return Err(anyhow::anyhow!(
            "DeepSeek API Key 未设置，请在配置页面填写 API Key（以 sk- 开头）"
        ));
    }

    let request_body = build_request(api_config, messages, options);
    log::debug!(
        "DeepSeek 请求: model={} max_tokens={} temp={} thinking={} json={} msg_count={}",
        api_config.model,
        request_body.max_tokens,
        request_body.temperature,
        api_config.thinking,
        options.json_mode,
        request_body.messages.len()
    );
    if api_config.thinking {
        log::debug!(
            "思考模式已开启，temperature={} 将被服务端忽略",
            api_config.temperature
        );
    }

    let url = format!(
        "{}/chat/completions",
        api_config.base_url.trim_end_matches('/')
    );
    let auth = format!("Bearer {}", api_key);

    let text = policy
        .run("DeepSeek 生成回复", || async {
            let resp = client
                .post(&url)
                .header("Authorization", auth.clone())
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(options.timeout_secs.max(1)))
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

    let result: ChatResponse = serde_json::from_str(&text).context("DeepSeek API 响应解析失败")?;

    let content = result
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| anyhow::anyhow!("DeepSeek 返回空回复"))?;

    let usage = result
        .usage
        .map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
        })
        .unwrap_or_default();

    Ok(ChatResult {
        text: content,
        usage,
        model: result.model.unwrap_or_else(|| api_config.model.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeepseekConfig;

    fn opts() -> ChatOptions {
        ChatOptions {
            max_tokens: 800,
            temperature: 0.7,
            json_mode: false,
            timeout_secs: 30,
        }
    }

    #[test]
    fn test_normalize_api_key() {
        assert_eq!(normalize_api_key("sk-abc"), "sk-abc");
        assert_eq!(normalize_api_key("  sk-abc "), "sk-abc");
        assert_eq!(normalize_api_key("abc123"), "sk-abc123");
        assert_eq!(normalize_api_key(""), "");
        assert_eq!(normalize_api_key("   "), "");
    }

    /// 请求体必须在默认情况下显式关闭思考模式。
    ///
    /// 线上模型的默认值是开启（effort = high），若不显式声明就会退回到
    /// 思考模式：temperature 被忽略、思维链占用输出预算、回复变慢。
    #[test]
    fn test_request_body_disables_thinking_by_default() {
        let cfg = DeepseekConfig::default();
        assert!(!cfg.thinking, "默认应为关闭思考模式");

        let body = build_request(&cfg, &[], opts());
        let json = serde_json::to_string(&body).expect("应能序列化请求体");
        assert!(
            json.contains(r#""thinking":{"type":"disabled"}"#),
            "请求体应显式关闭思考模式: {}",
            json
        );
        assert!(
            !json.contains("response_format"),
            "普通回复不应请求 JSON 模式"
        );
    }

    #[test]
    fn test_request_body_enables_thinking_when_configured() {
        let cfg = DeepseekConfig {
            thinking: true,
            ..Default::default()
        };
        let body = build_request(&cfg, &[], opts());
        let json = serde_json::to_string(&body).expect("应能序列化请求体");
        assert!(
            json.contains(r#""thinking":{"type":"enabled"}"#),
            "请求体应声明开启思考模式: {}",
            json
        );
    }

    #[test]
    fn test_json_mode_sets_response_format() {
        let cfg = DeepseekConfig::default();
        let mut options = opts();
        options.json_mode = true;
        let body = build_request(&cfg, &[], options);
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""response_format":{"type":"json_object"}"#));
    }

    /// 旧配置没有 thinking 字段时，必须回落到「关闭」而不是退回服务端默认
    #[test]
    fn test_legacy_config_without_thinking_field_defaults_to_disabled() {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            deepseek: DeepseekConfig,
        }
        let w: Wrapper =
            toml::from_str("[deepseek]\napi_key = \"sk-x\"\nmodel = \"deepseek-flash\"\n").unwrap();
        assert!(!w.deepseek.thinking, "缺字段时应回落到关闭");
    }
}
