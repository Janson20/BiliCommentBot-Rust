/// 评论回复与点赞模块
///
/// 对标 Python 版 reply_comment + like_comment + like_video + check_is_follower
use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::app_sign;
use crate::bvid;
use crate::http_client;

/// 回复 B站 评论（支持楼中楼）
///
/// - root_id: 根评论ID（楼中楼场景为根评论）
/// - parent_id: 父评论ID（楼中楼场景为直接父评论）
/// 返回：Ok(None) 成功，Ok(Some(错误信息)) B站拒绝，Err(_) 网络错误
pub async fn reply_comment(
    client: &reqwest::Client,
    bvid_str: &str,
    comment_id: &str,
    content: &str,
    csrf_token: &str,
    root_id: Option<&str>,
    parent_id: Option<&str>,
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

    let resp = client
        .post("https://api.bilibili.com/x/v2/reply/add")
        .form(&form)
        .send()
        .await
        .context("回复评论请求失败")?;

    let text = resp.text().await?;
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

    let resp = client
        .post("https://api.bilibili.com/x/v2/reply/action")
        .form(&form)
        .send()
        .await?;

    let text = resp.text().await?;
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    Ok(json["code"].as_i64().unwrap_or(-1) == 0)
}

/// 获取用户最新视频（APP API）
pub async fn get_user_latest_video(
    client: &reqwest::Client,
    uid: &str,
) -> Result<Option<(String, String)>> {
    // (bvid, title)
    let mut params = HashMap::from([
        ("vmid".to_string(), uid.to_string()),
        ("ps".to_string(), "1".to_string()),
        ("pn".to_string(), "1".to_string()),
        ("order".to_string(), "pubdate".to_string()),
        ("sort".to_string(), "desc".to_string()),
    ]);
    for (k, v) in http_client::app_common_params() {
        params.insert(k.to_string(), v.to_string());
    }
    let signed = app_sign::sign_from_map(params);

    let resp = client
        .get("https://app.bilibili.com/x/v2/space/archive/cursor")
        .query(&signed)
        .header("User-Agent", http_client::random_app_ua())
        .send()
        .await?;

    let text = resp.text().await?;
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

/// 点赞视频（APP API）
pub async fn like_video(client: &reqwest::Client, vid: &str) -> Result<bool> {
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

    let resp = client
        .post("https://app.bilibili.com/x/v2/view/like")
        .form(&signed)
        .header("User-Agent", http_client::random_app_ua())
        .send()
        .await?;

    let text = resp.text().await?;
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

/// 检查 follower_uid 是否关注了 following_uid（即 follower 是否为 following 的粉丝）
pub async fn check_is_follower(
    client: &reqwest::Client,
    follower_uid: &str,
    following_uid: &str,
) -> Result<bool> {
    let params = [
        ("vmid", following_uid),
        ("mid", follower_uid),
    ];

    let resp = client
        .get("https://api.bilibili.com/x/relation/same/followers")
        .query(&params)
        .send()
        .await?;

    let text = resp.text().await?;
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();

    if json["code"].as_i64() == Some(0) {
        Ok(json["data"]["following"].as_bool().unwrap_or(false))
    } else {
        Ok(false)
    }
}
