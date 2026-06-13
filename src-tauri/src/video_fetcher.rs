/// 视频列表获取（APP 端 API + 缓存）
///
/// 对标 Python 版 get_video_list + save/load_video_cache
use anyhow::{Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::app_sign;
use crate::http_client;

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

/// 获取视频列表（APP API + 本地缓存），返回视频列表
pub async fn get_video_list(
    client: &reqwest::Client,
    uid: &str,
    max_pages: u32,
    cache_file: &PathBuf,
    cache_expire_secs: u64,
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

        // 页间延迟
        if pn > 1 {
            let delay = 3.0 + rand::thread_rng().gen_range(0.0..1.5);
            tokio::time::sleep(tokio::time::Duration::from_secs_f64(delay)).await;
        }

        let resp = client
            .get(url)
            .query(&signed)
            .header("User-Agent", http_client::random_app_ua())
            .send()
            .await
            .context("获取视频列表请求失败")?;

        let text = resp.text().await.context("读取视频列表响应失败")?;
        let json: serde_json::Value =
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);

        let code = json["code"].as_i64().unwrap_or(-1);
        if code == 0 {
            let items = json["data"]["item"].as_array();
            if let Some(items) = items {
                if items.is_empty() {
                    break;
                }
                for item in items {
                    all_videos.push(VideoInfo {
                        bvid: item["bvid"].as_str().unwrap_or("").to_string(),
                        title: item["title"].as_str().unwrap_or("").to_string(),
                        desc: item["description"].as_str().unwrap_or("").to_string(),
                        play: item["play"].as_u64().unwrap_or(0),
                        comment: item["comment"].as_u64().unwrap_or(0),
                    });
                }
                log::info!(
                    "第{}页获取到{}个视频，累计{}个",
                    pn,
                    items.len(),
                    all_videos.len()
                );
                let has_more = json["data"]["has_next"].as_u64().unwrap_or(0);
                if has_more == 1 && items.len() >= 20 {
                    pn += 1;
                    continue;
                }
            }
        } else {
            let err_msg = json["message"].as_str().unwrap_or("");
            log::error!(
                "获取视频列表第{}页失败: code={} msg={}",
                pn,
                code,
                err_msg
            );
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
    if now - cache.fetch_time < expire_secs {
        Ok(cache.videos)
    } else {
        log::info!("视频缓存已过期");
        Ok(Vec::new())
    }
}

fn load_cache_raw(path: &PathBuf) -> Result<Vec<VideoInfo>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    let cache: VideoCache = serde_json::from_str(&content)?;
    Ok(cache.videos)
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
