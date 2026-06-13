/// 评论获取模块（主评论 + 楼中院子评论递归）
///
/// 对标 Python 版 get_video_comments + get_comment_replies
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::bvid;

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

/// 获取视频所有评论（含楼中楼子评论）
pub async fn get_video_comments(
    client: &reqwest::Client,
    bvid: &str,
    max_pages: u32,
    chained_reply: bool,
    max_reply_depth: u32,
) -> Result<Vec<Comment>> {
    let aid = bvid::bvid_to_aid(bvid).ok_or_else(|| anyhow::anyhow!("无法转换BVID: {}", bvid))?;
    let url = "https://api.bilibili.com/x/v2/reply";

    let mut all_comments: Vec<Comment> = Vec::new();
    let mut pn = 1u32;
    let mut page_size = 20u32;

    while pn <= max_pages {
        let params = [
            ("type", "1"),
            ("oid", &aid),
            ("pn", &pn.to_string()),
            ("ps", &page_size.to_string()),
            ("sort", "2"),
        ];

        let resp = client
            .get(url)
            .query(&params)
            .send()
            .await
            .context("获取评论请求失败")?;

        let text = resp.text().await.context("读取评论响应失败")?;
        let json: serde_json::Value =
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);

        let code = json["code"].as_i64().unwrap_or(-1);
        if code != 0 {
            let err = json["message"].as_str().unwrap_or("");
            // 对标 Python: 某些旧视频 page_size=20 会报错 "ps out of bounds"
            // 将 page_size 降级为 10 并重试当前页
            if err.contains("ps out of bounds") && pn == 1 && page_size > 10 {
                page_size = 10;
                log::warn!("page_size 20 超出范围，降级为 10 重试");
                continue;
            }
            break;
        }

        let replies = json["data"]["replies"].as_array();
        match replies {
            Some(replies) if !replies.is_empty() => {
                for r in replies {
                    let main_id = r["rpid"].to_string();

                    let mut main_clone = Comment {
                        comment_id: main_id.clone(),
                        content: r["content"]["message"].as_str().unwrap_or("").to_string(),
                        user: r["member"]["uname"].as_str().unwrap_or("").to_string(),
                        uid: r["member"]["mid"].to_string(),
                        ctime: r["ctime"].as_i64().unwrap_or(0),
                        parent_id: None,
                        root_id: None,
                        depth: 0,
                        children: Vec::new(),
                    };

                    // 获取子评论
                    if chained_reply && max_reply_depth > 0 {
                        let children = get_replies_recursive(
                            client,
                            aid.clone(),
                            main_id.clone(),
                            1,
                            max_reply_depth,
                        )
                        .await
                        .unwrap_or_default();
                        if !children.is_empty() {
                            log::info!("评论 {} 有 {} 条子评论", main_id, children.len());
                            main_clone.children = children.clone();
                            all_comments.extend(children);
                        }
                    }

                    all_comments.push(main_clone);
                }

                let page_info = &json["data"]["page"];
                let count = page_info["count"].as_u64().unwrap_or(0);
                if count <= (pn as u64) * (page_size as u64) {
                    break;
                }
                pn += 1;
            }
            _ => break,
        }
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
fn get_replies_recursive(
    client: &reqwest::Client,
    aid: String,
    root_id: String,
    current_depth: u32,
    max_depth: u32,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Comment>>> + Send + '_>> {
    Box::pin(async move {
        if current_depth > max_depth {
            return Ok(Vec::new());
        }

        let mut all: Vec<Comment> = Vec::new();
        let mut pn = 1u32;

        loop {
            let params = [
                ("type", "1"),
                ("oid", aid.as_str()),
                ("root", root_id.as_str()),
                ("pn", &pn.to_string()),
                ("ps", "10"),
            ];

            let resp = client
                .get("https://api.bilibili.com/x/v2/reply/reply")
                .query(&params)
                .send()
                .await?;

            let text = resp.text().await?;
            let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();

            if json["code"].as_i64().unwrap_or(-1) != 0 {
                break;
            }

            let replies = json["data"]["replies"].as_array();
            if let Some(replies) = replies {
                if replies.is_empty() {
                    break;
                }
                for r in replies {
                    let child_id = r["rpid"].to_string();

                    let mut child = Comment {
                        comment_id: child_id.clone(),
                        content: r["content"]["message"].as_str().unwrap_or("").to_string(),
                        user: r["member"]["uname"].as_str().unwrap_or("").to_string(),
                        uid: r["member"]["mid"].to_string(),
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
                        )
                        .await
                        .unwrap_or_default();
                    }

                    all.push(child);
                }

                let count = json["data"]["page"]["count"].as_u64().unwrap_or(0);
                if count <= (pn as u64) * 10 {
                    break;
                }
                pn += 1;
            } else {
                break;
            }
        }

        Ok(all)
    })
}
