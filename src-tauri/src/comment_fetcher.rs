/// 评论获取模块（主评论 + 楼中院子评论递归）
///
/// 对标 Python 版 `get_video_comments` + `get_comment_replies`。
///
/// 与 Python 版对齐的三个要点（早期端口有偏差）：
/// 1. **顺序**：主评论先入列表，其后才是它的子评论 —— 这样 `max_process`
///    的额度会先花在主评论上，而不是被楼中楼抢光。
/// 2. **去重**：用 `seen_ids` 跨页、跨楼层去重（B站分页会因新增评论而漂移，
///    同一条评论可能出现在两页里）。
/// 3. **深度**：传给递归的是 `max_reply_depth - 1`，与 Python 一致。
use std::collections::HashSet;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::bvid;
use crate::rate_limiter::{self, RetryPolicy};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub comment_id: String,
    pub content: String,
    pub user: String,
    pub uid: String,
    /// B站时间戳（秒）
    pub ctime: i64,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub root_id: Option<String>,
    #[serde(default)]
    pub depth: u32,
    #[serde(default)]
    pub children: Vec<Comment>,
}

/// 从 JSON 值里取 ID 字符串。
///
/// 优先读 `*_str` 字段；数字形式走 `as_u64`/`as_i64`，避免超大整数被 serde_json
/// 解析成 f64 后 `to_string()` 变成 `1.23e18` 这种科学计数法。
fn json_id(value: &serde_json::Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    if let Some(n) = value.as_u64() {
        return n.to_string();
    }
    if let Some(n) = value.as_i64() {
        return n.to_string();
    }
    String::new()
}

/// 取评论 ID：B站同时返回 `rpid`（数字）与 `rpid_str`（字符串），优先用后者
fn comment_id_of(item: &serde_json::Value) -> String {
    let via_str = json_id(&item["rpid_str"]);
    if !via_str.is_empty() {
        return via_str;
    }
    json_id(&item["rpid"])
}

/// 取用户 UID：优先 `mid_str`
fn uid_of(item: &serde_json::Value) -> String {
    let via_str = json_id(&item["member"]["mid_str"]);
    if !via_str.is_empty() {
        return via_str;
    }
    json_id(&item["member"]["mid"])
}

/// 带重试的 GET，返回响应文本；被限流/5xx 时抛出可重试错误。
async fn get_text_with_retry(
    client: &reqwest::Client,
    url: &str,
    params: &[(&str, &str)],
    label: &str,
    policy: RetryPolicy,
) -> Result<String> {
    policy
        .run(label, || async {
            let resp = client
                .get(url)
                .query(params)
                .send()
                .await
                .map_err(|e| rate_limiter::retryable(format!("请求失败: {}", e), None))?;

            let status = resp.status();
            let retry_after = rate_limiter::parse_retry_after(resp.headers());
            let text = resp
                .text()
                .await
                .map_err(|e| rate_limiter::retryable(format!("读取响应失败: {}", e), None))?;

            if rate_limiter::is_retryable_status(status)
                || rate_limiter::is_bili_rate_limited_text(&text, status)
            {
                return Err(rate_limiter::retryable(
                    format!(
                        "HTTP {} {}",
                        status.as_u16(),
                        text.chars().take(160).collect::<String>()
                    ),
                    retry_after,
                ));
            }
            Ok(text)
        })
        .await
}

/// 获取视频所有评论（含楼中楼子评论）
pub async fn get_video_comments(
    client: &reqwest::Client,
    bvid: &str,
    max_pages: u32,
    chained_reply: bool,
    max_reply_depth: u32,
    policy: RetryPolicy,
) -> Result<Vec<Comment>> {
    let aid = bvid::bvid_to_aid(bvid).ok_or_else(|| anyhow::anyhow!("无法转换BVID: {}", bvid))?;
    let url = "https://api.bilibili.com/x/v2/reply";

    let mut all_comments: Vec<Comment> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut pn = 1u32;
    let mut page_size = 20u32;

    while pn <= max_pages {
        let pn_str = pn.to_string();
        let ps_str = page_size.to_string();
        let params = [
            ("type", "1"),
            ("oid", aid.as_str()),
            ("pn", pn_str.as_str()),
            ("ps", ps_str.as_str()),
            ("sort", "2"),
        ];

        let text = match get_text_with_retry(
            client,
            url,
            &params,
            &format!("获取评论第{}页", pn),
            policy,
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                // 单页失败不该丢掉之前已经拿到的评论
                log::error!("获取评论第{}页失败: {}", pn, e);
                if all_comments.is_empty() {
                    return Err(e).context("获取评论请求失败");
                }
                break;
            }
        };

        let json: serde_json::Value =
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json["code"].as_i64().unwrap_or(-1);

        if code != 0 {
            let err = json["message"].as_str().unwrap_or("");
            // 对标 Python: 某些旧视频 page_size=20 会报错 "ps out of bounds"
            if err.contains("ps out of bounds") && pn == 1 && page_size > 10 {
                page_size = 10;
                log::warn!("page_size 20 超出范围，降级为 10 重试");
                continue;
            }
            log::error!("获取评论第{}页失败: code={} msg={}", pn, code, err);
            break;
        }

        let Some(replies) = json["data"]["replies"].as_array() else {
            break;
        };
        if replies.is_empty() {
            break;
        }

        let raw_len = replies.len();
        for r in replies {
            let main_id = comment_id_of(r);
            if main_id.is_empty() || !seen_ids.insert(main_id.clone()) {
                continue;
            }

            let mut main_comment = Comment {
                comment_id: main_id.clone(),
                content: r["content"]["message"].as_str().unwrap_or("").to_string(),
                user: r["member"]["uname"].as_str().unwrap_or("").to_string(),
                uid: uid_of(r),
                ctime: r["ctime"].as_i64().unwrap_or(0),
                parent_id: None,
                root_id: None,
                depth: 0,
                children: Vec::new(),
            };

            // 先取子评论，再按 [主评论, 子评论...] 的顺序写入列表：
            // 与 Python 一致，保证 max_process 的额度优先用在主评论上，
            // 而不是被楼中楼抢光。
            let mut children = Vec::new();
            if chained_reply && max_reply_depth > 0 {
                // Python 传的是 max_reply_depth - 1
                let depth_budget = max_reply_depth.saturating_sub(1);
                children = get_replies_recursive(
                    client,
                    aid.clone(),
                    main_id.clone(),
                    1,
                    depth_budget,
                    &mut seen_ids,
                    policy,
                )
                .await
                .unwrap_or_default();
                if !children.is_empty() {
                    log::debug!("评论 {} 有 {} 条子评论", main_id, children.len());
                }
            }

            main_comment.children = children.clone();
            all_comments.push(main_comment);
            all_comments.extend(children);
        }

        // 分页终止判定：优先用 page.count，缺失时退回「本页不满一页」的判定，
        // 避免数据里没有 count 字段时只读第一页。
        let page_count = json["data"]["page"]["count"].as_u64();
        match page_count {
            Some(count) => {
                if count <= (pn as u64) * (page_size as u64) {
                    break;
                }
            }
            None => {
                if (raw_len as u32) < page_size {
                    break;
                }
            }
        }
        pn += 1;
    }

    if chained_reply {
        let main_count = all_comments.iter().filter(|c| c.depth == 0).count();
        log::info!(
            "共获取 {} 条主评论和 {} 条子评论",
            main_count,
            all_comments.len() - main_count
        );
    }

    Ok(all_comments)
}

/// 递归获取子评论（使用 Box::pin 解决 async 递归问题）
#[allow(clippy::too_many_arguments)]
fn get_replies_recursive<'a>(
    client: &'a reqwest::Client,
    aid: String,
    root_id: String,
    current_depth: u32,
    max_depth: u32,
    seen_ids: &'a mut HashSet<String>,
    policy: RetryPolicy,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Comment>>> + Send + 'a>> {
    Box::pin(async move {
        if current_depth > max_depth {
            return Ok(Vec::new());
        }

        let mut all: Vec<Comment> = Vec::new();
        let mut pn = 1u32;

        loop {
            let pn_str = pn.to_string();
            let params = [
                ("type", "1"),
                ("oid", aid.as_str()),
                ("root", root_id.as_str()),
                ("pn", pn_str.as_str()),
                ("ps", "10"),
            ];

            let text = match get_text_with_retry(
                client,
                "https://api.bilibili.com/x/v2/reply/reply",
                &params,
                &format!("获取子评论 root={}", root_id),
                policy,
            )
            .await
            {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("获取子评论失败 root={}: {}", root_id, e);
                    break;
                }
            };

            let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if json["code"].as_i64().unwrap_or(-1) != 0 {
                break;
            }

            let Some(replies) = json["data"]["replies"].as_array() else {
                break;
            };
            if replies.is_empty() {
                break;
            }

            let raw_len = replies.len();
            for r in replies {
                let child_id = comment_id_of(r);
                if child_id.is_empty() || !seen_ids.insert(child_id.clone()) {
                    continue;
                }

                let mut child = Comment {
                    comment_id: child_id.clone(),
                    content: r["content"]["message"].as_str().unwrap_or("").to_string(),
                    user: r["member"]["uname"].as_str().unwrap_or("").to_string(),
                    uid: uid_of(r),
                    ctime: r["ctime"].as_i64().unwrap_or(0),
                    parent_id: Some(root_id.clone()),
                    root_id: Some(root_id.clone()),
                    depth: current_depth,
                    children: Vec::new(),
                };

                if current_depth < max_depth {
                    child.children = get_replies_recursive(
                        client,
                        aid.clone(),
                        child_id.clone(),
                        current_depth + 1,
                        max_depth,
                        seen_ids,
                        policy,
                    )
                    .await
                    .unwrap_or_default();
                }

                all.push(child);
            }

            let count = json["data"]["page"]["count"].as_u64();
            match count {
                Some(count) => {
                    if count <= (pn as u64) * 10 {
                        break;
                    }
                }
                None => {
                    if raw_len < 10 {
                        break;
                    }
                }
            }
            pn += 1;
        }

        Ok(all)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_comment_id_prefers_str_field() {
        let item = json!({"rpid": 12345678901234567890u64, "rpid_str": "12345678901234567890"});
        assert_eq!(comment_id_of(&item), "12345678901234567890");
    }

    #[test]
    fn test_comment_id_falls_back_to_number() {
        let item = json!({"rpid": 42});
        assert_eq!(comment_id_of(&item), "42");
    }

    #[test]
    fn test_comment_id_never_scientific_notation() {
        // 走数字分支时必须用整数格式化，不能出现 1.2e18
        let item = json!({"rpid": 1234567890123456789u64});
        let id = comment_id_of(&item);
        assert!(!id.contains('e') && !id.contains('E'), "id={}", id);
        assert!(id.chars().all(|c| c.is_ascii_digit()), "id={}", id);
    }

    #[test]
    fn test_uid_prefers_mid_str() {
        let item = json!({"member": {"mid": 7, "mid_str": "7"}});
        assert_eq!(uid_of(&item), "7");
        let item2 = json!({"member": {"mid": 12345}});
        assert_eq!(uid_of(&item2), "12345");
    }

    #[test]
    fn test_json_id_empty_when_missing() {
        assert_eq!(json_id(&json!(null)), "");
        assert_eq!(json_id(&json!({"a": 1})), "");
    }
}
