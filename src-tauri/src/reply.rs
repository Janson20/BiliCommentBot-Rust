/// 评论回复与点赞模块
///
/// 对标 Python 版 reply_comment + like_comment + like_video + get_user_latest_video
/// + check_is_follower
///
/// 关于 Cookie：Python 版用 `requests.Session`，会话 Cookie 会**自动附加到每一个
/// 请求**（包括 `app.bilibili.com` 与 `api.bilibili.com`）。Rust 端没有填充
/// reqwest 的 cookie jar，因此这里必须显式带上 `Cookie` 头 —— 否则点赞用户视频、
/// 检查粉丝关系这类需要登录的接口会静默失败。
use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::app_sign;
use crate::bvid;
use crate::http_client;
use crate::rate_limiter::{self, RetryPolicy};

/// 回复 B站 评论（支持楼中楼）
///
/// - root_id: 根评论ID（楼中楼场景为根评论）
/// - parent_id: 父评论ID（楼中楼场景为**被回复的那条评论**）
/// - cookie_str: 完整 Cookie 字符串（必须含 SESSDATA 等会话凭证）
///
/// 返回：Ok(None) 成功，Ok(Some(错误信息)) B站拒绝，Err(_) 网络错误
#[allow(clippy::too_many_arguments)]
pub async fn reply_comment(
    client: &reqwest::Client,
    bvid_str: &str,
    comment_id: &str,
    content: &str,
    csrf_token: &str,
    root_id: Option<&str>,
    parent_id: Option<&str>,
    cookie_str: &str,
    policy: RetryPolicy,
) -> Result<Option<String>> {
    let aid = bvid::bvid_to_aid(bvid_str)
        .ok_or_else(|| anyhow::anyhow!("无法转换BVID: {}", bvid_str))?;

    let root = root_id.unwrap_or(comment_id);
    let parent = parent_id.unwrap_or(comment_id);

    let form = [
        ("type", "1"),
        ("oid", &aid),
        ("rpid", comment_id),
        ("root", root),
        ("parent", parent),
        ("message", content),
        ("csrf", csrf_token),
    ];

    let text = policy
        .run("回复评论", || async {
            let resp = client
                .post("https://api.bilibili.com/x/v2/reply/add")
                .header("Cookie", cookie_str)
                .header("Origin", "https://www.bilibili.com")
                .header("Referer", "https://www.bilibili.com/")
                .form(&form)
                .send()
                .await
                .map_err(|e| rate_limiter::retryable(format!("请求失败: {}", e), None))?;

            let status = resp.status();
            let retry_after = rate_limiter::parse_retry_after(resp.headers());
            let text = resp
                .text()
                .await
                .map_err(|e| rate_limiter::retryable(format!("读取响应失败: {}", e), None))?;

            if rate_limiter::is_bili_rate_limited_text(&text, status)
                || rate_limiter::is_retryable_status(status)
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
        .context("回复评论请求失败")?;

    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let code = json["code"].as_i64().unwrap_or(-1);
    if code == 0 {
        log::info!("回复评论 {} 成功", comment_id);
        Ok(None)
    } else {
        let msg = json["message"].as_str().unwrap_or("未知错误").to_string();
        log::warn!("回复评论 {} 失败: code={} msg={}", comment_id, code, msg);
        Ok(Some(format!("code={} msg={}", code, msg)))
    }
}

/// 点赞评论
pub async fn like_comment(
    client: &reqwest::Client,
    bvid_str: &str,
    comment_id: &str,
    csrf_token: &str,
    cookie_str: &str,
    policy: RetryPolicy,
) -> Result<bool> {
    let aid = bvid::bvid_to_aid(bvid_str)
        .ok_or_else(|| anyhow::anyhow!("无法转换BVID: {}", bvid_str))?;

    let form = [
        ("type", "1"),
        ("oid", &aid),
        ("rpid", comment_id),
        ("action", "1"),
        ("csrf", csrf_token),
    ];

    let text = policy
        .run("点赞评论", || async {
            let resp = client
                .post("https://api.bilibili.com/x/v2/reply/action")
                .header("Cookie", cookie_str)
                .header("Origin", "https://www.bilibili.com")
                .header("Referer", "https://www.bilibili.com/")
                .form(&form)
                .send()
                .await
                .map_err(|e| rate_limiter::retryable(format!("请求失败: {}", e), None))?;

            let status = resp.status();
            let retry_after = rate_limiter::parse_retry_after(resp.headers());
            let text = resp
                .text()
                .await
                .map_err(|e| rate_limiter::retryable(format!("读取响应失败: {}", e), None))?;

            if rate_limiter::is_bili_rate_limited_text(&text, status)
                || rate_limiter::is_retryable_status(status)
            {
                return Err(rate_limiter::retryable(
                    format!("HTTP {} {}", status.as_u16(), text.chars().take(160).collect::<String>()),
                    retry_after,
                ));
            }
            Ok(text)
        })
        .await?;

    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        log::debug!(
            "点赞评论 {} 未成功: code={} msg={}",
            comment_id,
            code,
            json["message"].as_str().unwrap_or("")
        );
    }
    Ok(code == 0)
}

/// 获取用户最新视频（APP API）。返回 (bvid, title)
///
/// 与 Python 版一致地带上会话 Cookie：`requests.Session` 会把 `.bilibili.com`
/// 域下的 Cookie 一并发往 `app.bilibili.com`。
pub async fn get_user_latest_video(
    client: &reqwest::Client,
    uid: &str,
    cookie_str: &str,
    policy: RetryPolicy,
) -> Result<Option<(String, String)>> {
    let text = fetch_app_archive_page(client, uid, "1", cookie_str, policy).await?;
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();

    if json["code"].as_i64() == Some(0) {
        if let Some(items) = json["data"]["item"].as_array() {
            if let Some(item) = items.first() {
                let bvid = item["bvid"].as_str().unwrap_or("").to_string();
                let title = item["title"].as_str().unwrap_or("").to_string();
                if !bvid.is_empty() {
                    return Ok(Some((bvid, title)));
                }
            }
        }
    }
    Ok(None)
}

/// 请求 APP 端投稿列表（签名 + 会话 Cookie + 重试）
pub async fn fetch_app_archive_page(
    client: &reqwest::Client,
    uid: &str,
    ps: &str,
    cookie_str: &str,
    policy: RetryPolicy,
) -> Result<String> {
    let mut params = HashMap::from([
        ("vmid".to_string(), uid.to_string()),
        ("ps".to_string(), ps.to_string()),
        ("pn".to_string(), "1".to_string()),
        ("order".to_string(), "pubdate".to_string()),
        ("sort".to_string(), "desc".to_string()),
    ]);
    for (k, v) in http_client::app_common_params() {
        params.insert(k.to_string(), v.to_string());
    }
    let signed = app_sign::sign_from_map(params);
    let ua = http_client::random_app_ua();

    policy
        .run("获取用户投稿", || async {
            let mut req = client
                .get("https://app.bilibili.com/x/v2/space/archive/cursor")
                .query(&signed)
                .header("User-Agent", ua);
            if !cookie_str.is_empty() {
                req = req.header("Cookie", cookie_str);
            }

            let resp = req
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
                    format!("HTTP {} {}", status.as_u16(), text.chars().take(160).collect::<String>()),
                    retry_after,
                ));
            }
            Ok(text)
        })
        .await
}

/// 点赞视频（APP API）
pub async fn like_video(
    client: &reqwest::Client,
    vid: &str,
    cookie_str: &str,
    policy: RetryPolicy,
) -> Result<bool> {
    let aid = bvid::bvid_to_aid(vid)
        .ok_or_else(|| anyhow::anyhow!("无法转换BVID: {}", vid))?;

    let mut params = HashMap::from([
        ("aid".to_string(), aid),
        ("like".to_string(), "1".to_string()),
    ]);
    for (k, v) in http_client::app_common_params() {
        params.insert(k.to_string(), v.to_string());
    }
    let signed = app_sign::sign_from_map(params);
    let ua = http_client::random_app_ua();

    let text = policy
        .run("点赞视频", || async {
            let mut req = client
                .post("https://app.bilibili.com/x/v2/view/like")
                .form(&signed)
                .header("User-Agent", ua);
            if !cookie_str.is_empty() {
                req = req.header("Cookie", cookie_str);
            }

            let resp = req
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
                    format!("HTTP {} {}", status.as_u16(), text.chars().take(160).collect::<String>()),
                    retry_after,
                ));
            }
            Ok(text)
        })
        .await?;

    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let code = json["code"].as_i64().unwrap_or(-1);
    if code == 0 {
        log::info!("点赞视频 {} 成功", vid);
    } else {
        log::warn!(
            "点赞视频 {} 失败: code={} msg={}",
            vid,
            code,
            json["message"].as_str().unwrap_or("")
        );
    }
    Ok(code == 0)
}

/// 检查 `follower_uid` 是否关注了 `following_uid`。
///
/// 注意参数方向（对标 `bot.py` 的 `check_is_follower`）：
/// **第一个参数是被检查关注状态的人，第二个参数是被关注的人**。
/// 「仅点赞关注了我的用户」应调用 `check_is_follower(评论者uid, 我的uid)`，
/// 即询问「评论者有没有关注我」。早期端口把这两个参数写反了，
/// 结果问的是「我有没有关注评论者」，功能等于失效。
pub async fn check_is_follower(
    client: &reqwest::Client,
    follower_uid: &str,
    following_uid: &str,
    cookie_str: &str,
    policy: RetryPolicy,
) -> Result<bool> {
    let params = [("vmid", following_uid), ("mid", follower_uid)];

    let text = policy
        .run("检查粉丝关系", || async {
            let mut req = client
                .get("https://api.bilibili.com/x/relation/same/followers")
                .query(&params)
                .header("Referer", "https://www.bilibili.com/");
            if !cookie_str.is_empty() {
                req = req.header("Cookie", cookie_str);
            }

            let resp = req
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
                    format!("HTTP {} {}", status.as_u16(), text.chars().take(160).collect::<String>()),
                    retry_after,
                ));
            }
            Ok(text)
        })
        .await?;

    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if json["code"].as_i64() == Some(0) {
        Ok(json["data"]["following"].as_bool().unwrap_or(false))
    } else {
        // 接口失败时不能谎报"不是粉丝"：调用方把它当作 false 会静默跳过点赞，
        // 这里返回 Err 让调用方记录明确原因。
        Err(anyhow::anyhow!(
            "检查粉丝关系失败: code={} msg={}",
            json["code"].as_i64().unwrap_or(-1),
            json["message"].as_str().unwrap_or("未知错误")
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::rate_limiter::RetryPolicy;

    #[test]
    fn test_retry_policy_default_is_used_for_signatures() {
        // 仅确保 RetryPolicy 可 Copy 传参（编译期约束）
        let p = RetryPolicy::default();
        let q = p;
        assert_eq!(p.max_retries, q.max_retries);
    }
}
