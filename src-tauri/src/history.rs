/// 历史记录管理 (SQLite)
///
/// 对标 Python 版 history.json + processed_comments
/// 使用 SQLite 替代 JSON 文件存储，首次启动自动迁移旧数据
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ════════════════════════════════════════════════════════════════
//  数据模型
// ════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub comment_id: String,
    pub bvid: String,
    pub video_title: String,
    pub content: String,
    pub user: String,
    pub uid: String,
    pub time: i64,
    pub reply_time: i64,
    pub reply_content: String,
    pub timestamp: String,
    pub parent_id: Option<String>,
    pub root_id: Option<String>,
    pub depth: u32,
}

// ════════════════════════════════════════════════════════════════
//  HistoryManager
// ════════════════════════════════════════════════════════════════

const JSON_FILE: &str = "history.json";
const JSON_BAK: &str = "history.json.bak";

pub struct HistoryManager {
    conn: Mutex<Connection>,
}

impl HistoryManager {
    /// 新建 SQLite 历史管理器。自动建表 + 从 JSON 迁移（如果存在）
    pub fn new(db_path: &Path) -> Self {
        let conn = Connection::open(db_path).expect("无法打开 history.db");
        // WAL 模式提升并发性能
        conn.execute_batch("PRAGMA journal_mode=WAL;").ok();
        // 建表
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                comment_id  TEXT NOT NULL UNIQUE,
                bvid        TEXT NOT NULL DEFAULT '',
                video_title TEXT NOT NULL DEFAULT '',
                content     TEXT NOT NULL DEFAULT '',
                user        TEXT NOT NULL DEFAULT '',
                uid         TEXT NOT NULL DEFAULT '',
                time        INTEGER NOT NULL DEFAULT 0,
                reply_time  INTEGER NOT NULL DEFAULT 0,
                reply_content TEXT NOT NULL DEFAULT '',
                timestamp   TEXT NOT NULL DEFAULT '',
                parent_id   TEXT,
                root_id     TEXT,
                depth       INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_history_comment_id ON history(comment_id);
            CREATE INDEX IF NOT EXISTS idx_history_bvid ON history(bvid);
            CREATE INDEX IF NOT EXISTS idx_history_time ON history(time);",
        )
        .expect("无法创建 history 表");

        let hm = Self {
            conn: Mutex::new(conn),
        };

        // 自动迁移旧 JSON → SQLite
        hm.migrate_from_json_if_needed();

        hm
    }

    // ── 自动迁移 ──

    fn migrate_from_json_if_needed(&self) {
        let json_path = PathBuf::from(JSON_FILE);
        let bak_path = PathBuf::from(JSON_BAK);

        if !json_path.exists() {
            return;
        }

        // 如果已有备份文件，说明迁移已完成，跳过
        if bak_path.exists() {
            log::info!("history.json.bak 已存在，跳过迁移");
            return;
        }

        // 检查 DB 中是否已有数据
        let count: i64 = self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap_or(0);

        if count > 0 {
            log::info!("数据库已有 {} 条记录，跳过 JSON 迁移", count);
            // 备份 JSON 文件
            if let Err(e) = std::fs::rename(&json_path, &bak_path) {
                log::error!("重命名 history.json 失败: {}", e);
            }
            return;
        }

        // 读取并导入 JSON
        match std::fs::read_to_string(&json_path) {
            Ok(content) => match serde_json::from_str::<Vec<HistoryEntry>>(&content) {
                Ok(entries) => {
                    let total = entries.len();
                    let conn = self.conn.lock().unwrap();
                    let tx = conn.unchecked_transaction().unwrap();
                    {
                        let mut stmt = tx
                            .prepare(
                                "INSERT OR IGNORE INTO history
                                (comment_id, bvid, video_title, content, user, uid,
                                 time, reply_time, reply_content, timestamp,
                                 parent_id, root_id, depth)
                                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                            )
                            .unwrap();
                        for e in &entries {
                            stmt.execute(params![
                                e.comment_id,
                                e.bvid,
                                e.video_title,
                                e.content,
                                e.user,
                                e.uid,
                                e.time,
                                e.reply_time,
                                e.reply_content,
                                e.timestamp,
                                e.parent_id,
                                e.root_id,
                                e.depth,
                            ])
                            .ok();
                        }
                    }
                    tx.commit().unwrap();
                    log::info!("已从 history.json 迁移 {} 条记录到 SQLite", total);
                }
                Err(e) => {
                    log::error!("解析 history.json 失败: {}", e);
                    return;
                }
            },
            Err(e) => {
                log::error!("读取 history.json 失败: {}", e);
                return;
            }
        }

        // 备份原文件
        if let Err(e) = std::fs::rename(&json_path, &bak_path) {
            log::error!("重命名 history.json 失败: {}", e);
        } else {
            log::info!("history.json 已备份为 history.json.bak");
        }
    }

    // ── 写入 ──

    pub fn add(
        &self,
        comment_id: &str,
        bvid: &str,
        video_title: &str,
        content: &str,
        user: &str,
        uid: &str,
        ctime: i64,
        reply_content: &str,
        parent_id: Option<&str>,
        root_id: Option<&str>,
        depth: u32,
    ) {
        let reply_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO history
            (comment_id, bvid, video_title, content, user, uid,
             time, reply_time, reply_content, timestamp,
             parent_id, root_id, depth)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                comment_id,
                bvid,
                video_title,
                content,
                user,
                uid,
                ctime,
                reply_time,
                reply_content,
                timestamp,
                parent_id,
                root_id,
                depth,
            ],
        )
        .ok();
    }

    // ── 查询 ──

    pub fn is_processed(&self, comment_id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM history WHERE comment_id = ?1",
            params![comment_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false)
    }

    pub fn total_replied(&self) -> u64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM history", [], |row| row.get::<_, i64>(0))
            .map(|c| c as u64)
            .unwrap_or(0)
    }

    /// 分页查询（按时间倒序）
    pub fn query_paginated(
        &self,
        page: u32,
        page_size: u32,
    ) -> (u32, Vec<HistoryEntry>) {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap_or(0);

        let offset = ((page.saturating_sub(1)) * page_size) as i64;
        let limit = page_size as i64;

        let mut stmt = conn
            .prepare(
                "SELECT comment_id, bvid, video_title, content, user, uid,
                        time, reply_time, reply_content, timestamp,
                        parent_id, root_id, depth
                 FROM history
                 ORDER BY reply_time DESC
                 LIMIT ?1 OFFSET ?2",
            )
            .unwrap();

        let entries = stmt
            .query_map(params![limit, offset], |row| {
                Ok(HistoryEntry {
                    comment_id: row.get(0)?,
                    bvid: row.get(1)?,
                    video_title: row.get(2)?,
                    content: row.get(3)?,
                    user: row.get(4)?,
                    uid: row.get(5)?,
                    time: row.get(6)?,
                    reply_time: row.get(7)?,
                    reply_content: row.get(8)?,
                    timestamp: row.get(9)?,
                    parent_id: row.get(10)?,
                    root_id: row.get(11)?,
                    depth: row.get(12)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        (total as u32, entries)
    }

    /// 按 bvid 分组查询（用于历史页面卡片视图）
    pub fn query_grouped(&self) -> Vec<(String, String, Vec<HistoryEntry>)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT comment_id, bvid, video_title, content, user, uid,
                        time, reply_time, reply_content, timestamp,
                        parent_id, root_id, depth
                 FROM history
                 ORDER BY reply_time DESC",
            )
            .unwrap();

        let entries: Vec<HistoryEntry> = stmt
            .query_map([], |row| {
                Ok(HistoryEntry {
                    comment_id: row.get(0)?,
                    bvid: row.get(1)?,
                    video_title: row.get(2)?,
                    content: row.get(3)?,
                    user: row.get(4)?,
                    uid: row.get(5)?,
                    time: row.get(6)?,
                    reply_time: row.get(7)?,
                    reply_content: row.get(8)?,
                    timestamp: row.get(9)?,
                    parent_id: row.get(10)?,
                    root_id: row.get(11)?,
                    depth: row.get(12)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // 保留 bvid 出现顺序（倒序），同 bvid 合并
        let mut seen = HashSet::new();
        let mut groups: Vec<(String, String, Vec<HistoryEntry>)> = Vec::new();

        for entry in entries {
            if !seen.contains(&entry.bvid) {
                seen.insert(entry.bvid.clone());
                groups.push((
                    entry.bvid.clone(),
                    entry.video_title.clone(),
                    Vec::new(),
                ));
            }
            if let Some(group) = groups.iter_mut().find(|(b, _, _)| b == &entry.bvid) {
                group.2.push(entry);
            }
        }

        groups
    }

    // ── 删除 ──

    pub fn clear(&self) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM history", []).ok();
        // VACUUM 回收磁盘空间
        conn.execute("VACUUM", []).ok();
        log::info!("已清除 SQLite 历史记录");
    }

    /// 从旧版 JSON 文件导入（用于 Python 项目迁移）
    pub fn import_from_json(&self, json_path: &Path) -> Result<u32, String> {
        if !json_path.exists() {
            return Err(format!("文件不存在: {:?}", json_path));
        }
        let content =
            std::fs::read_to_string(json_path).map_err(|e| format!("读取失败: {}", e))?;
        let entries: Vec<HistoryEntry> =
            serde_json::from_str(&content).map_err(|e| format!("解析失败: {}", e))?;

        let total = entries.len() as u32;
        let conn = self.conn.lock().unwrap();
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("事务失败: {}", e))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO history
                    (comment_id, bvid, video_title, content, user, uid,
                     time, reply_time, reply_content, timestamp,
                     parent_id, root_id, depth)
                    VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                )
                .map_err(|e| format!("准备语句失败: {}", e))?;
            for e in &entries {
                stmt.execute(params![
                    e.comment_id,
                    e.bvid,
                    e.video_title,
                    e.content,
                    e.user,
                    e.uid,
                    e.time,
                    e.reply_time,
                    e.reply_content,
                    e.timestamp,
                    e.parent_id,
                    e.root_id,
                    e.depth,
                ])
                .ok();
            }
        }
        tx.commit().map_err(|e| format!("提交失败: {}", e))?;
        log::info!("从 JSON 导入 {} 条历史记录到 SQLite", total);
        Ok(total)
    }
}

// ════════════════════════════════════════════════════════════════
//  测试
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let tmp = std::env::temp_dir().join("test_history_basic.db");
        let _ = std::fs::remove_file(&tmp);
        let hm = HistoryManager::new(&tmp);

        assert_eq!(hm.total_replied(), 0);
        assert!(!hm.is_processed("cmt_1"));

        hm.add("cmt_1", "BV001", "Test Video", "hello", "user1", "123", 1000, "reply1", None, None, 0);
        assert_eq!(hm.total_replied(), 1);
        assert!(hm.is_processed("cmt_1"));

        let (total, items) = hm.query_paginated(1, 10);
        assert_eq!(total, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].comment_id, "cmt_1");

        hm.clear();
        assert_eq!(hm.total_replied(), 0);
        assert!(!hm.is_processed("cmt_1"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_deduplication() {
        let tmp = std::env::temp_dir().join("test_history_dedup.db");
        let _ = std::fs::remove_file(&tmp);
        let hm = HistoryManager::new(&tmp);

        hm.add("cmt_1", "BV001", "", "", "", "", 0, "", None, None, 0);
        hm.add("cmt_1", "BV001", "", "", "", "", 0, "", None, None, 0); // duplicate
        assert_eq!(hm.total_replied(), 1);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_group_query() {
        let tmp = std::env::temp_dir().join("test_history_group.db");
        let _ = std::fs::remove_file(&tmp);
        let hm = HistoryManager::new(&tmp);

        hm.add("c1", "BV_A", "Video A", "hi", "u1", "1", 100, "r1", None, None, 0);
        hm.add("c2", "BV_B", "Video B", "hi", "u2", "2", 200, "r2", None, None, 0);
        hm.add("c3", "BV_A", "Video A", "hi", "u3", "3", 300, "r3", None, None, 0);

        let groups = hm.query_grouped();
        assert_eq!(groups.len(), 2);
        // BV_B 的 reply_time 更大，先出现
        assert_eq!(groups[0].0, "BV_B");
        assert_eq!(groups[0].2.len(), 1);
        assert_eq!(groups[1].0, "BV_A");
        assert_eq!(groups[1].2.len(), 2);

        let _ = std::fs::remove_file(&tmp);
    }
}
