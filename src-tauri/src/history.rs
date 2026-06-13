/// 历史记录管理 (SQLite)
///
/// 对标 Python 版 history.json + processed_comments
/// 使用 SQLite 替代 JSON 文件存储，首次启动自动迁移旧数据
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ──────────────────────────────────────────────────────────────────
//  数据模型
// ──────────────────────────────────────────────────────────────────

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

// ──────────────────────────────────────────────────────────────────
//  HistoryManager
// ──────────────────────────────────────────────────────────────────

const JSON_FILE: &str = "history.json";
const JSON_BAK: &str = "history.json.bak";

pub struct HistoryManager {
    conn: Mutex<Connection>,
}

impl HistoryManager {
    /// New SQLite history manager. Auto-create tables + migrate from JSON if present.
    pub fn new(db_path: &Path) -> Self {
        let conn = Connection::open(db_path).expect("Cannot open history.db");
        // WAL mode for better concurrent performance
        conn.execute_batch("PRAGMA journal_mode=WAL;").ok();
        // Create tables
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
        .expect("Cannot create history table");

        let hm = Self {
            conn: Mutex::new(conn),
        };

        // Auto-migrate from old JSON if present
        hm.migrate_from_json_if_needed();

        hm
    }

    /// WAL checkpoint: flush WAL to main DB file (call before clearing data)
    pub fn checkpoint(&self) {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").ok();
        log::info!("WAL checkpoint completed");
    }

    // ── Auto-migration ──

    fn migrate_from_json_if_needed(&self) {
        let json_path = PathBuf::from(JSON_FILE);
        let bak_path = PathBuf::from(JSON_BAK);

        if !json_path.exists() {
            return;
        }

        // Skip if backup already exists (migration already done)
        if bak_path.exists() {
            log::info!("history.json.bak exists, skipping migration");
            return;
        }

        // Check if DB already has data
        let count: i64 = self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap_or(0);

        if count > 0 {
            log::info!("DB already has {} records, skipping JSON migration", count);
            // Backup JSON file
            if let Err(e) = std::fs::rename(&json_path, &bak_path) {
                log::error!("Failed to rename history.json: {}", e);
            }
            return;
        }

        // Read and import JSON
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
                    log::info!("Migrated {} records from history.json to SQLite", total);
                }
                Err(e) => {
                    log::error!("Failed to parse history.json: {}", e);
                    return;
                }
            },
            Err(e) => {
                log::error!("Failed to read history.json: {}", e);
                return;
            }
        }

        // Backup original file
        if let Err(e) = std::fs::rename(&json_path, &bak_path) {
            log::error!("Failed to rename history.json: {}", e);
        } else {
            log::info!("history.json backed up as history.json.bak");
        }
    }

    // ── Write ──

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

    // ── Query ──

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

    /// Paginated query (ordered by reply_time DESC)
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

    /// Grouped by bvid (for history page card view)
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

        // Preserve bvid appearance order (descending by reply_time), merge same bvid
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

    // ── Delete ──

    pub fn clear(&self) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM history", []).ok();
        // VACUUM to reclaim disk space
        conn.execute("VACUUM", []).ok();
        log::info!("SQLite history cleared");
    }

    /// Import from legacy JSON file (for Python project migration)
    pub fn import_from_json(&self, json_path: &Path) -> Result<u32, String> {
        if !json_path.exists() {
            return Err(format!("File not found: {:?}", json_path));
        }
        let content =
            std::fs::read_to_string(json_path).map_err(|e| format!("Read failed: {}", e))?;
        let entries: Vec<HistoryEntry> =
            serde_json::from_str(&content).map_err(|e| format!("Parse failed: {}", e))?;

        let total = entries.len() as u32;
        let conn = self.conn.lock().unwrap();
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("Transaction failed: {}", e))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO history
                    (comment_id, bvid, video_title, content, user, uid,
                     time, reply_time, reply_content, timestamp,
                     parent_id, root_id, depth)
                    VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                )
                .map_err(|e| format!("Prepare failed: {}", e))?;
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
        tx.commit().map_err(|e| format!("Commit failed: {}", e))?;
        log::info!("Imported {} history records from JSON to SQLite", total);
        Ok(total)
    }
}

// ──────────────────────────────────────────────────────────────────
//  Tests
// ──────────────────────────────────────────────────────────────────

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
        // BV_B has larger reply_time, appears first
        assert_eq!(groups[0].0, "BV_B");
        assert_eq!(groups[0].2.len(), 1);
        assert_eq!(groups[1].0, "BV_A");
        assert_eq!(groups[1].2.len(), 2);

        let _ = std::fs::remove_file(&tmp);
    }
}
