/// 历史记录管理
///
/// 对标 Python 版 history.json + processed_comments
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub comment_id: String,
    #[serde(default)]
    pub bvid: String,
    pub content: String,
    pub user: String,
    pub uid: String,
    pub time: i64,
    pub reply_time: i64,
    pub reply_content: String,
    pub timestamp: String,
}

#[derive(Debug)]
pub struct HistoryManager {
    file_path: PathBuf,
    pub processed_ids: HashSet<String>,
    pub entries: Vec<HistoryEntry>,
}

impl HistoryManager {
    pub fn new(file_path: PathBuf) -> Self {
        let mut hm = Self {
            file_path,
            processed_ids: HashSet::new(),
            entries: Vec::new(),
        };
        hm.load();
        hm
    }

    fn load(&mut self) {
        if !self.file_path.exists() {
            return;
        }
        match fs::read_to_string(&self.file_path) {
            Ok(content) => {
                if let Ok(entries) = serde_json::from_str::<Vec<HistoryEntry>>(&content) {
                    for entry in &entries {
                        self.processed_ids.insert(entry.comment_id.clone());
                    }
                    self.entries = entries;
                    log::info!("加载历史记录，已处理 {} 条评论", self.processed_ids.len());
                }
            }
            Err(e) => {
                log::error!("加载历史记录失败: {}", e);
            }
        }
    }

    pub fn add(
        &mut self,
        comment_id: &str,
        bvid: &str,
        content: &str,
        user: &str,
        uid: &str,
        ctime: i64,
        reply_content: &str,
    ) {
        let entry = HistoryEntry {
            comment_id: comment_id.to_string(),
            bvid: bvid.to_string(),
            content: content.to_string(),
            user: user.to_string(),
            uid: uid.to_string(),
            time: ctime,
            reply_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            reply_content: reply_content.to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };

        self.processed_ids.insert(comment_id.to_string());
        self.entries.push(entry);

        // 持久化到文件
        match serde_json::to_string_pretty(&self.entries) {
            Ok(content) => {
                if let Some(parent) = self.file_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Err(e) = fs::write(&self.file_path, &content) {
                    log::error!("保存历史记录失败: {}", e);
                }
            }
            Err(e) => log::error!("序列化历史记录失败: {}", e),
        }
    }

    pub fn is_processed(&self, comment_id: &str) -> bool {
        self.processed_ids.contains(comment_id)
    }

    pub fn clear(&mut self) {
        self.processed_ids.clear();
        self.entries.clear();
        if self.file_path.exists() {
            let _ = fs::remove_file(&self.file_path);
        }
        log::info!("已清除历史记录");
    }

    pub fn total_replied(&self) -> u64 {
        self.processed_ids.len() as u64
    }
}
