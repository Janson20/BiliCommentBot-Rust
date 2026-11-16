/// OpenAI 兼容端点客户端
///
/// 只依赖标准的 /chat/completions 与 /models：
/// - OpenAI
/// - OpenRouter / SiliconFlow / Moonshot / 智谱 等兼容网关
/// - LM Studio / vLLM / llama.cpp server 等本地服务
///
/// API Key 为空时不发送 Authorization 头（本地服务通常不需要）。
use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::ai::{ChatOptions, ChatResult, TokenUsage};
use crate::config::OpenAiConfig;
use crate::prompt::ChatMessage;
use crate::rate_limiter::{self, RetryPolicy};

fn endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn build_body(cfg: &OpenAiConfig, messages: &[ChatMessage], options: ChatOptions) -> Value {
    let mut body = json!({
        "model": cfg.model,
        "messages": messages,
    });
    // 新版推理模型通常不接受自定义 temperature；设置 reasoning_effort 时省略该字段
    if cfg.reasoning_effort.trim().is_empty() {
        body["temperature"] = json!(options.temperature);
    }

    let token_param = cfg.token_param.trim();
    let token_field = if token_param == "max_completion_tokens" {
        "max_completion_tokens"
    } else {
        "max_tokens"
    };
    body[token_field] = json!(options.max_tokens);

    if options.json_mode {
        body["response_format"] = json!({ "type": "json_object" });
    }
    let effort = cfg.reasoning_effort.trim();
    if !effort.is_empty() {
        body["reasoning_effort"] = json!(effort);
    }
    body
}

fn apply_auth(req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
    if api_key.trim().is_empty() {
        req
    } else {
        req.header("Authorization", format!("Bearer {}", api_key.trim()))
    }
}

/// 调用 OpenAI 兼容端点的 /chat/completions
pub async fn chat(
    client: &reqwest::Client,
    cfg: &OpenAiConfig,
    messages: &[ChatMessage],
    options: ChatOptions,
    policy: RetryPolicy,
) -> Result<ChatResult> {
    if cfg.base_url.trim().is_empty() {
        return Err(anyhow::anyhow!("OpenAI 兼容端点未填写 base_url"));
    }
    if cfg.model.trim().is_empty() {
        return Err(anyhow::anyhow!("OpenAI 兼容端点未填写模型名"));
    }

    let body = build_body(cfg, messages, options);
    let url = endpoint(&cfg.base_url, "chat/completions");
    log::debug!(
        "OpenAI 兼容请求: url={} model={} max_tokens={} temp={} json={} effort={}",
        url,
        cfg.model,
        options.max_tokens,
        options.temperature,
        options.json_mode,
        cfg.reasoning_effort
    );

    let api_key = cfg.api_key.clone();
    let text = policy
        .run("OpenAI 兼容生成回复", || {
            let body = body.clone();
            let url = url.clone();
            let api_key = api_key.clone();
            async move {
                let req = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .timeout(std::time::Duration::from_secs(options.timeout_secs.max(1)))
                    .json(&body);
                let resp = apply_auth(req, &api_key)
                    .send()
                    .await
                    .map_err(|e| rate_limiter::retryable(format!("网络错误: {}", e), None))?;

                let status = resp.status();
                let retry_after = rate_limiter::parse_retry_after(resp.headers());
                let text = resp
                    .text()
                    .await
                    .map_err(|e| rate_limiter::retryable(format!("读取响应失败: {}", e), None))?;

                if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                    return Err(rate_limiter::retryable(
                        format!(
                            "HTTP {} {}",
                            status.as_u16(),
                            text.chars().take(200).collect::<String>()
                        ),
                        retry_after,
                    ));
                }
                if !status.is_success() {
                    return Err(anyhow::anyhow!(
                        "OpenAI 兼容 API 错误 {}: {}",
                        status.as_u16(),
                        text.chars().take(300).collect::<String>()
                    ));
                }
                Ok(text)
            }
        })
        .await
        .context("OpenAI 兼容 API 请求失败")?;

    let result: Value = serde_json::from_str(&text).context("OpenAI 兼容 API 响应解析失败")?;

    let content = result["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    if content.is_empty() {
        // 不能把 reasoning_content 当作公开回复：思维链可能包含系统提示词或内部推理
        return Err(anyhow::anyhow!(
            "OpenAI 兼容端点返回空正文（模型可能只输出了 reasoning_content）"
        ));
    }

    Ok(ChatResult {
        text: content,
        usage: parse_usage(&result),
        model: result["model"].as_str().unwrap_or(&cfg.model).to_string(),
    })
}

fn parse_usage(result: &Value) -> TokenUsage {
    TokenUsage {
        prompt_tokens: result["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: result["usage"]["completion_tokens"].as_u64().unwrap_or(0),
    }
}

/// 拉取 /models 列表
pub async fn list_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>> {
    if base_url.trim().is_empty() {
        return Err(anyhow::anyhow!("未填写 base_url，无法获取模型列表"));
    }
    let url = endpoint(base_url, "models");
    let resp = apply_auth(client.get(&url), api_key)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("获取模型列表失败")?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "获取模型列表失败 HTTP {}: {}",
            status.as_u16(),
            text.chars().take(200).collect::<String>()
        ));
    }
    let json: Value = serde_json::from_str(&text).context("模型列表响应解析失败")?;

    let mut models = Vec::new();
    if let Some(items) = json["data"].as_array() {
        for item in items {
            if let Some(id) = item["id"].as_str() {
                models.push(id.to_string());
            }
        }
    }
    // 部分兼容网关返回 {"models": ["a", "b"]} 或 {"models":[{"name":...}]}
    if models.is_empty() {
        if let Some(items) = json["models"].as_array() {
            for item in items {
                if let Some(id) = item.as_str() {
                    models.push(id.to_string());
                } else if let Some(id) = item["id"].as_str().or_else(|| item["name"].as_str()) {
                    models.push(id.to_string());
                }
            }
        }
    }
    models.sort();
    models.dedup();
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> OpenAiConfig {
        OpenAiConfig {
            api_key: "test-key".into(),
            base_url: "https://example.com/v1/".into(),
            model: "test-model".into(),
            ..Default::default()
        }
    }

    fn opts() -> ChatOptions {
        ChatOptions {
            max_tokens: 321,
            temperature: 0.4,
            json_mode: true,
            timeout_secs: 30,
        }
    }

    #[test]
    fn test_endpoint_joins_without_double_slash() {
        assert_eq!(
            endpoint("https://example.com/v1/", "chat/completions"),
            "https://example.com/v1/chat/completions"
        );
        assert_eq!(
            endpoint("http://127.0.0.1:1234/v1", "/models"),
            "http://127.0.0.1:1234/v1/models"
        );
    }

    #[test]
    fn test_body_defaults_to_max_tokens() {
        let body = build_body(&cfg(), &[ChatMessage::user("hi")], opts());
        assert_eq!(body["max_tokens"], 321);
        assert!(body.get("max_completion_tokens").is_none());
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["temperature"], 0.4);
    }

    #[test]
    fn test_body_supports_max_completion_tokens_and_effort() {
        let mut c = cfg();
        c.token_param = "max_completion_tokens".into();
        c.reasoning_effort = "high".into();
        let body = build_body(&c, &[], opts());
        assert_eq!(body["max_completion_tokens"], 321);
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["reasoning_effort"], "high");
        assert!(
            body.get("temperature").is_none(),
            "推理模型不应强制发送 temperature"
        );
    }

    #[test]
    fn test_usage_and_model_are_mapped() {
        let value: Value = serde_json::json!({
            "model": "m-1",
            "choices": [{"message": {"content": " 你好 "}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 4}
        });
        let usage = parse_usage(&value);
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 4);
        assert_eq!(
            value["choices"][0]["message"]["content"]
                .as_str()
                .unwrap()
                .trim(),
            "你好"
        );
    }
}
