//! 单实例保护
//!
//! 同时跑两个实例会有两个机器人共用同一份 `history.db` 与 Cookie：去重是
//! 「先查再写」，两个进程可能同时查到"未处理"从而对同一条评论各回一次，
//! 在公开评论区里表现为重复回复。
//!
//! 实现方式：对数据目录下的 `app.lock` 取一个**操作系统级文件锁**。
//! 进程退出（包括崩溃）时由操作系统自动释放，因此不会留下需要人工清理的
//! 残留锁文件，也不需要记录 / 校验 PID。
//!
//! 策略是「失败开放」：只有明确拿到 `WouldBlock`（确实已被占用）才判定为
//! 重复启动；其它任何错误（如目录只读）都放行，避免因为锁机制本身的问题
//! 让程序完全无法启动。

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;

/// 锁文件名
pub const LOCK_FILE_NAME: &str = "app.lock";

static GUARD: OnceLock<Mutex<Option<File>>> = OnceLock::new();

fn lock_path() -> PathBuf {
    crate::paths::data_dir().join(LOCK_FILE_NAME)
}

/// 对指定路径取锁的结果
#[derive(Debug)]
enum LockOutcome {
    /// 拿到锁，句柄需一直持有到进程结束
    Acquired(File),
    /// 已被其它持有者占用
    AlreadyRunning,
    /// 锁机制本身不可用（例如目录只读），应当放行
    Unavailable(String),
}

fn lock_at(path: &Path) -> LockOutcome {
    let file = match OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
    {
        Ok(f) => f,
        Err(e) => {
            return LockOutcome::Unavailable(format!("无法创建锁文件 {:?}: {}", path, e));
        }
    };

    match file.try_lock() {
        Ok(()) => LockOutcome::Acquired(file),
        Err(std::fs::TryLockError::WouldBlock) => LockOutcome::AlreadyRunning,
        Err(std::fs::TryLockError::Error(e)) => {
            LockOutcome::Unavailable(format!("获取锁失败: {}", e))
        }
    }
}

/// 尝试获取单实例锁。
///
/// * 返回 `true` —— 可以继续启动（本次是唯一实例，或锁机制不可用但选择放行）
/// * 返回 `false` —— 已有实例在运行，调用方应提示用户然后退出
pub fn acquire() -> bool {
    let holder = GUARD.get_or_init(|| Mutex::new(None));
    let mut guard = match holder.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    if guard.is_some() {
        // 本次进程内已经拿过锁
        return true;
    }

    let path = lock_path();
    match lock_at(&path) {
        LockOutcome::Acquired(file) => {
            log::debug!("已获得单实例锁: {:?}", path);
            *guard = Some(file);
            true
        }
        LockOutcome::AlreadyRunning => {
            log::warn!("单实例锁已被其它进程持有: {:?}", path);
            false
        }
        LockOutcome::Unavailable(reason) => {
            log::warn!("{}；跳过单实例检查继续启动", reason);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_lock(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("bili_single_instance_tests");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn test_first_lock_is_acquired_and_creates_file() {
        let path = temp_lock("first.lock");
        match lock_at(&path) {
            LockOutcome::Acquired(_file) => {}
            other => panic!("首次取锁应成功，实际: {:?}", other),
        }
        assert!(path.exists(), "锁文件应被创建: {:?}", path);
        let _ = std::fs::remove_file(&path);
    }

    /// 第二个独立句柄（相当于第二个实例）必须拿不到锁
    #[test]
    fn test_second_holder_is_rejected() {
        let path = temp_lock("second.lock");

        let first = match lock_at(&path) {
            LockOutcome::Acquired(f) => f,
            other => panic!("首次取锁应成功，实际: {:?}", other),
        };

        match lock_at(&path) {
            LockOutcome::AlreadyRunning => {}
            other => panic!("第二个持有者应被拒绝，实际: {:?}", other),
        }

        // 释放后应可再次获取
        drop(first);
        match lock_at(&path) {
            LockOutcome::Acquired(_) => {}
            other => panic!("释放后应能再次取锁，实际: {:?}", other),
        }
        let _ = std::fs::remove_file(&path);
    }

    /// 锁机制不可用时必须放行，不能让程序起不来
    #[test]
    fn test_unavailable_path_fails_open() {
        let bogus = PathBuf::from("Z:\\definitely\\not\\a\\real\\dir\\app.lock");
        match lock_at(&bogus) {
            LockOutcome::Unavailable(_) => {}
            LockOutcome::Acquired(_) => {
                // 少数环境下 Z: 恰好可写，此时也算通过
            }
            LockOutcome::AlreadyRunning => panic!("不存在的路径不应被判定为已在运行"),
        }
    }

    /// 进程内重复调用是幂等的
    #[test]
    fn test_acquire_is_idempotent_within_process() {
        let first = acquire();
        let second = acquire();
        assert_eq!(first, second, "同进程内重复获取结果应一致");
    }
}

