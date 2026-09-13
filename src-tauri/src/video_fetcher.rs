/// 视频列表获取（APP 端 API + 缓存）
///
/// 对标 Python 版 get_video_list + save/load_video_cache
use anyhow::Result;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::app_sign;
use crate::http_client;
use crate::rate_limiter::{self, RetryPolicy};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub bvid: String,
    pub title: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub play: u64,
    #[serde(default)]
    pub comment: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VideoCache {
    videos: Vec<VideoInfo>,
    fetch_time: u64,
    fetch_timestamp: String,
}

/// 从 APP API 的条目里取值：优先 `stat.*`，回落到顶层字段（对标 Python）
fn stat_u64(item: &serde_json::Value, stat_key: &str, flat_key: &str) -> u64 {
    item["stat"][stat_key]
        .as_u64()
        .or_else(|| item[flat_key].as_u64())
        .unwrap_or(0)
}

fn parse_items(json: &serde_json::Value) -> Option<Vec<VideoInfo>> {
    let items = json["data"]["item"].as_array()?;
    Some(
        items
            .iter()
            .map(|item| {
                let title = item["title"].as_str().unwrap_or("").to_string();
                // 简介缺失时回落到标题：否则送给 AI 的视频上下文是空的
                let desc = item["description"]
                    .as_str()
                    .filter(|d| !d.is_empty())
                    .unwrap_or(&title)
                    .to_string();
                VideoInfo {
                    bvid: item["bvid"].as_str().unwrap_or("").to_string(),
                    title,
                    desc,
                    play: stat_u64(item, "view", "play"),
                    comment: stat_u64(item, "reply", "comment"),
                }
            })
            .collect(),
    )
}

/// 获取视频列表（APP API + 本地缓存），返回视频列表
pub async fn get_video_list(
    client: &reqwest::Client,
    uid: &str,
    max_pages: u32,
    cache_file: &PathBuf,
    cache_expire_secs: u64,
    policy: RetryPolicy,
) -> Result<Vec<VideoInfo>> {
    // 检查缓存
    if let Ok(cached) = load_cache(cache_file, cache_expire_secs) {
        if !cached.is_empty() {
            log::info!("使用视频缓存，共 {} 个", cached.len());
            return Ok(cached);
        }
    }

    log::info!("重新获取视频列表（APP API）...");
    let mut all_videos: Vec<VideoInfo> = Vec::new();
    let mut pn = 1u32;
    let url = "https://app.bilibili.com/x/v2/space/archive/cursor";
    let ua = http_client::random_app_ua();

    while pn <= max_pages {
        let mut params = HashMap::from([
            ("vmid".to_string(), uid.to_string()),
            ("ps".to_string(), "20".to_string()),
            ("pn".to_string(), pn.to_string()),
            ("order".to_string(), "pubdate".to_string()),
            ("sort".to_string(), "desc".to_string()),
        ]);
        // 添加 APP 公共参数
        for (k, v) in http_client::app_common_params() {
            params.insert(k.to_string(), v.to_string());
        }
        let signed = app_sign::sign_from_map(params);

        // 页间延迟：不小于 3s，同时尊重配置的最小请求间隔（对标 Python）
        if pn > 1 {
            let base = (policy.min_interval * 1.2).max(3.0);
            let delay = base + rand::thread_rng().gen_range(0.0..1.5);
            tokio::time::sleep(tokio::time::Duration::from_secs_f64(delay)).await;
        }

        let fetched = policy
            .run("获取视频列表", || async {
                let resp = client
                    .get(url)
                    .query(&signed)
                    .header("User-Agent", ua)
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
            .await;

        let text = match fetched {
            Ok(t) => t,
            Err(e) => {
                log::error!("获取视频列表第{}页失败: {}", pn, e);
                // 已经有部分数据就先返回它，不要让整轮失败
                if !all_videos.is_empty() {
                    save_cache(cache_file, &all_videos);
                    return Ok(all_videos);
                }
                break;
            }
        };

        let json: serde_json::Value =
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json["code"].as_i64().unwrap_or(-1);

        if code == 0 {
            match parse_items(&json) {
                Some(items) if !items.is_empty() => {
                    let count = items.len();
                    all_videos.extend(items);
                    log::info!(
                        "第{}页获取到{}个视频，累计{}个",
                        pn,
                        count,
                        all_videos.len()
                    );
                    let has_more = json["data"]["has_next"].as_u64().unwrap_or(0);
                    if has_more == 1 && count >= 20 {
                        pn += 1;
                        continue;
                    }
                }
                _ => break,
            }
        } else {
            let err_msg = json["message"].as_str().unwrap_or("");
            log::error!(
                "获取视频列表第{}页失败: code={} msg={}",
                pn,
                code,
                err_msg
            );
            // 对标 Python: 若因频率限制中断且已有部分数据，保留已获取的视频
            let is_rate_limited =
                crate::rate_limiter::is_bili_rate_limited_text(&text, reqwest::StatusCode::OK);
            if is_rate_limited {
                if !all_videos.is_empty() {
                    log::warn!(
                        "视频列表获取被频率限制，保留已取得的 {} 个视频",
                        all_videos.len()
                    );
                    save_cache(cache_file, &all_videos);
                } else {
                    log::warn!("视频列表被频率限制且无已获取数据，将使用过期缓存");
                }
            }
        }
        break;
    }

    if all_videos.is_empty() {
        // 回退到过期缓存
        if let Ok(cached) = load_cache_raw(cache_file) {
            if !cached.is_empty() {
                log::warn!("获取视频列表失败，回退到过期缓存（{}个视频）", cached.len());
                return Ok(cached);
            }
        }
    }

    if !all_videos.is_empty() {
        save_cache(cache_file, &all_videos);
    }

    Ok(all_videos)
}

fn load_cache(path: &PathBuf, expire_secs: u64) -> Result<Vec<VideoInfo>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = load_cache_raw(path)?;
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    // 检查过期
    let content = fs::read_to_string(path)?;
    let cache: VideoCache = serde_json::from_str(&content)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now.saturating_sub(cache.fetch_time) < expire_secs {
        Ok(cache.videos)
    } else {
        log::info!("视频缓存已过期");
        Ok(Vec::new())
    }
}

/// 读取缓存文件；兼容 Python 版可能写入的「纯数组」旧格式
fn load_cache_raw(path: &PathBuf) -> Result<Vec<VideoInfo>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;

    if let Ok(cache) = serde_json::from_str::<VideoCache>(&content) {
        return Ok(cache.videos);
    }
    // 旧格式：直接是一个数组
    if let Ok(list) = serde_json::from_str::<Vec<VideoInfo>>(&content) {
        log::info!("视频缓存为旧版数组格式，已按新格式读取");
        return Ok(list);
    }
    Err(anyhow::anyhow!("视频缓存格式无法识别"))
}

fn save_cache(path: &PathBuf, videos: &[VideoInfo]) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cache = VideoCache {
        videos: videos.to_vec(),
        fetch_time: now,
        fetch_timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };
    if let Ok(content) = serde_json::to_string_pretty(&cache) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, &content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_items_prefers_stat_and_falls_back() {
        let payload = json!({
            "code": 0,
            "data": { "item": [
                {
                    "bvid": "BV1",
                    "title": "标题A",
                    "description": "简介A",
                    "play": 1,
                    "comment": 2,
                    "stat": { "view": 111, "reply": 22 }
                },
                {
                    "bvid": "BV2",
                    "title": "标题B",
                    "play": 7,
                    "comment": 8
                }
            ]}
        });

        let items = parse_items(&payload).expect("应解析出条目");
        assert_eq!(items.len(), 2);

        assert_eq!(items[0].play, 111, "优先用 stat.view");
        assert_eq!(items[0].comment, 22, "优先用 stat.reply");
        assert_eq!(items[0].desc, "简介A");

        // 没有 stat 时回落到顶层 play/comment
        assert_eq!(items[1].play, 7);
        assert_eq!(items[1].comment, 8);
        // 简介缺失时回落到标题，避免送给 AI 的视频上下文是空的
        assert_eq!(items[1].desc, "标题B");
    }

    #[test]
    fn test_parse_items_missing_item_is_none() {
        assert!(parse_items(&json!({"code": 0, "data": {}})).is_none());
    }

    #[test]
    fn test_legacy_array_cache_format_is_accepted() {
        let dir = std::env::temp_dir().join("bili_cache_format_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("video_cache.json");

        let legacy = json!([
            {"bvid": "BV1", "title": "t", "desc": "d", "play": 1, "comment": 2}
        ]);
        std::fs::write(&path, legacy.to_string()).unwrap();
        let loaded = load_cache_raw(&path).expect("旧版数组格式应可读取");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].bvid, "BV1");

        let _ = std::fs::remove_file(&path);
    }
}
