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
    #[serde(default)]
    pub bvid: String,
    #[serde(default)]
    pub video_title: String,
    pub content: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub uid: String,
    pub time: i64,
    pub reply_time: i64,
    pub reply_content: String,
    pub timestamp: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub root_id: Option<String>,
    #[serde(default)]
    pub depth: u32,
}

// ──────────────────────────────────────────────────────────────────
//  HistoryManager
// ──────────────────────────────────────────────────────────────────

/// 把一行查询结果映射成 [`HistoryEntry`]（三个查询共用，避免三份重复映射代码）
fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
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
}

const JSON_FILE: &str = "history.json";
const JSON_BAK: &str = "history.json.bak";

pub struct HistoryManager {
    conn: Mutex<Option<Connection>>,
    /// 数据库是否可用。不可用时所有写入被丢弃、`is_processed` 一律返回 true
    /// （宁可漏回复也不能重复回复），并且 `start_bot` 会拒绝启动。
    available: bool,
    db_path: PathBuf,
}

impl HistoryManager {
    /// New SQLite history manager. Auto-create tables + migrate from JSON if present.
    ///
    /// 打不开数据库时**不 panic**：把错误写进日志、标记为不可用，让程序仍能启动，
    /// 由调用方（`start_bot`）决定是否拒绝运行机器人。
    pub fn new(db_path: &Path) -> Self {
        let (conn, available) = match Self::open_connection(db_path) {
            Ok(conn) => (Some(conn), true),
            Err(e) => {
                log::error!(
                    "无法打开历史数据库 {:?}: {}。历史去重不可用，机器人将拒绝启动以避免重复回复。",
                    db_path,
                    e
                );
                (None, false)
            }
        };

        let hm = Self {
            conn: Mutex::new(conn),
            available,
            db_path: db_path.to_path_buf(),
        };

        // Auto-migrate from old JSON if present
        if hm.available {
            hm.migrate_from_json_if_needed();
        }

        hm
    }

    fn open_connection(db_path: &Path) -> Result<Connection, rusqlite::Error> {
        // SQLite 不会自动创建父目录
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        let conn = Connection::open(db_path)?;
        // WAL mode for better concurrent performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
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
        )?;

        Ok(conn)
    }

    /// 历史数据库是否可用
    pub fn available(&self) -> bool {
        self.available
    }

    /// 数据库路径（用于日志与错误提示）
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Close the connection: WAL checkpoint + drop. Must be called `&mut` to release lock.
    pub fn close(&mut self) {
        let mut guard = self.conn.lock().unwrap();
        if let Some(conn) = guard.take() {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").ok();
        }
        self.available = false;
        log::info!("SQLite connection closed");
    }

    /// Internal: lock and run `f` on the connection; returns `fallback` when the
    /// database is unavailable or already closed (never panics).
    fn with_conn<F, R>(&self, fallback: R, f: F) -> R
    where
        F: FnOnce(&Connection) -> R,
    {
        let guard = match self.conn.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.as_ref() {
            Some(conn) => f(conn),
            None => fallback,
        }
    }

    // ── Auto-migration ──

    fn migrate_from_json_if_needed(&self) {
        let json_path = crate::paths::resolve(JSON_FILE);
        let bak_path = crate::paths::resolve(JSON_BAK);

        if !json_path.exists() {
            return;
        }

        // Skip if backup already exists (migration already done)
        if bak_path.exists() {
            log::info!("history.json.bak 已存在，跳过 JSON 迁移");
            return;
        }

        // Check if DB already has data
        let count: i64 = self.with_conn(0, |conn| {
            conn.query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
                .unwrap_or(0)
        });

        if count > 0 {
            log::info!("数据库已有 {} 条记录，跳过 JSON 迁移", count);
            // Backup JSON file
            if let Err(e) = std::fs::rename(&json_path, &bak_path) {
                log::error!("备份 history.json 失败: {}", e);
            }
            return;
        }

        // Read and import JSON
        let content = match std::fs::read_to_string(&json_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("读取 history.json 失败: {}", e);
                return;
            }
        };
        let entries: Vec<HistoryEntry> = match serde_json::from_str(&content) {
            Ok(e) => e,
            Err(e) => {
                log::error!("解析 history.json 失败: {}", e);
                return;
            }
        };

        let total = entries.len();
        let imported = self.insert_many(&entries).unwrap_or(0);
        log::info!(
            "已从 history.json 迁移 {}/{} 条记录到 SQLite",
            imported,
            total
        );

        // Backup original file
        if let Err(e) = std::fs::rename(&json_path, &bak_path) {
            log::error!("备份 history.json 失败: {}", e);
        } else {
            log::info!("history.json 已备份为 history.json.bak");
        }
    }

    /// 批量插入（供 JSON 迁移复用），返回成功写入的行数
    fn insert_many(&self, entries: &[HistoryEntry]) -> Result<usize, String> {
        self.with_conn(Err("数据库不可用".to_string()), |conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("开启事务失败: {}", e))?;
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
                for e in entries {
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
            tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;
            Ok(entries.len())
        })
    }

    // ── Write ──

    /// 写入一条回复历史（去重依据：`comment_id` 唯一）
    #[allow(clippy::too_many_arguments)]
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

        self.with_conn((), |conn| {
            if let Err(e) = conn.execute(
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
            ) {
                log::warn!("写入历史记录失败 ({}): {}", comment_id, e);
            }
        });
    }

    // ── Query ──

    /// 这条评论是否已经处理过。
    ///
    /// 数据库不可用时返回 `true`：宁可漏回复，也不能因为读不到去重表而把同一批
    /// 评论重复回复一遍。
    pub fn is_processed(&self, comment_id: &str) -> bool {
        self.with_conn(true, |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM history WHERE comment_id = ?1",
                params![comment_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(true)
        })
    }

    pub fn total_replied(&self) -> u64 {
        self.with_conn(0, |conn| {
            conn.query_row("SELECT COUNT(*) FROM history", [], |row| row.get::<_, i64>(0))
                .map(|c| c as u64)
                .unwrap_or(0)
        })
    }

    /// Paginated query (ordered by reply_time DESC)
    pub fn query_paginated(
        &self,
        page: u32,
        page_size: u32,
    ) -> (u32, Vec<HistoryEntry>) {
        let total: i64 = self.with_conn(0, |conn| {
            conn.query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
                .unwrap_or(0)
        });

        let offset = ((page.saturating_sub(1)) * page_size) as i64;
        let limit = page_size as i64;

        let entries: Vec<HistoryEntry> = self.with_conn(Vec::new(), |conn| {
            let mut stmt = match conn.prepare(
                "SELECT comment_id, bvid, video_title, content, user, uid,
                            time, reply_time, reply_content, timestamp,
                            parent_id, root_id, depth
                     FROM history
                     ORDER BY reply_time DESC
                     LIMIT ?1 OFFSET ?2",
            ) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("准备历史分页查询失败: {}", e);
                    return Vec::new();
                }
            };

            let rows = stmt.query_map(params![limit, offset], row_to_entry);
            match rows {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    log::error!("历史分页查询失败: {}", e);
                    Vec::new()
                }
            }
        });

        (total as u32, entries)
    }

    /// Grouped by bvid (for history page card view)
    pub fn query_grouped(&self) -> Vec<(String, String, Vec<HistoryEntry>)> {
        let entries: Vec<HistoryEntry> = self.with_conn(Vec::new(), |conn| {
            let mut stmt = match conn.prepare(
                "SELECT comment_id, bvid, video_title, content, user, uid,
                            time, reply_time, reply_content, timestamp,
                            parent_id, root_id, depth
                     FROM history
                     ORDER BY reply_time DESC",
            ) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("准备历史查询失败: {}", e);
                    return Vec::new();
                }
            };

            let rows = stmt.query_map([], row_to_entry);
            match rows {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    log::error!("历史查询失败: {}", e);
                    Vec::new()
                }
            }
        });

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

    /// Query all entries flat, ordered by reply_time DESC (for date-based grouping)
    pub fn query_all_flat(&self) -> Vec<HistoryEntry> {
        self.with_conn(Vec::new(), |conn| {
            let mut stmt = match conn.prepare(
                "SELECT comment_id, bvid, video_title, content, user, uid,
                            time, reply_time, reply_content, timestamp,
                            parent_id, root_id, depth
                     FROM history
                     ORDER BY reply_time DESC",
            ) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("准备历史查询失败: {}", e);
                    return Vec::new();
                }
            };

            let rows = stmt.query_map([], row_to_entry);
            match rows {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    log::error!("历史查询失败: {}", e);
                    Vec::new()
                }
            }
        })
    }

    // ── Delete ──

    pub fn clear(&self) {
        self.with_conn((), |conn| {
            conn.execute("DELETE FROM history", []).ok();
            // VACUUM to reclaim disk space
            conn.execute("VACUUM", []).ok();
        });
        log::info!("SQLite history cleared");
    }

    /// Import from legacy JSON file (for Python project migration)
    pub fn import_from_json(&self, json_path: &Path) -> Result<u32, String> {
        if !json_path.exists() {
            return Err(format!("文件不存在: {:?}", json_path));
        }
        let content =
            std::fs::read_to_string(json_path).map_err(|e| format!("读取失败: {}", e))?;
        let entries: Vec<HistoryEntry> =
            serde_json::from_str(&content).map_err(|e| format!("解析失败: {}", e))?;

        let total = entries.len() as u32;
        self.insert_many(&entries)?;
        log::info!("已从 JSON 导入 {} 条历史记录到 SQLite", total);
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
        // c3(BV_A) reply_time=300 最大，BV_A 先出现
        assert_eq!(groups[0].0, "BV_A");
        assert_eq!(groups[0].2.len(), 2);
        assert_eq!(groups[1].0, "BV_B");
        assert_eq!(groups[1].2.len(), 1);

        let _ = std::fs::remove_file(&tmp);
    }
}
