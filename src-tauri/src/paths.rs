//! 运行时数据目录解析
//!
//! 所有运行时数据（`config.toml` / `history.db` / `bilibili_cookie.json` /
//! `video_cache.json` / `logs/`）统一放在**用户数据目录**下，而不再使用进程的
//! 当前工作目录（CWD）。
//!
//! 为什么必须这样做：
//!
//! * 通过快捷方式或注册表 `Run` 项启动时，CWD 是不可控的 —— 同一次安装可能读到
//!   两份不同的配置，甚至因为没有写权限而静默保存失败；
//! * 旧行为会把数据写进安装目录，卸载或升级即丢失；
//! * 开发模式（`npm run tauri dev`）的 CWD 是 `src-tauri/`，数据会混进源码目录
//!   —— 历史上真实的 `config.toml`（含 Cookie 与 API Key）正是因此被提交进了仓库。
//!
//! 目录优先级：
//!
//! 1. 环境变量 `BILICOMMENTBOT_DATA_DIR`（便于便携部署与测试）
//! 2. Windows：`%APPDATA%\BiliCommentBot-RS`
//! 3. 其它平台 / 无 `APPDATA`：可执行文件同级的 `BiliCommentBot-RS/`
//! 4. 兜底：`./BiliCommentBot-RS`

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 用户数据目录名
pub const APP_DIR_NAME: &str = "BiliCommentBot-RS";

/// 环境变量覆盖项
pub const DATA_DIR_ENV: &str = "BILICOMMENTBOT_DATA_DIR";

/// 需要随数据目录一起迁移 / 清理的运行时文件
pub const DATA_FILES: &[&str] = &[
    "config.toml",
    "bilibili_cookie.json",
    "history.db",
    "history.db-wal",
    "history.db-shm",
    "history.json",
    "history.json.bak",
    "video_cache.json",
];

/// 日志子目录名
pub const LOG_DIR_NAME: &str = "logs";

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 用户数据目录。首次调用时会尝试创建。
///
/// 创建失败时回退到当前目录，保证程序仍可启动（错误会写进日志）。
pub fn data_dir() -> PathBuf {
    DATA_DIR
        .get_or_init(|| {
            let dir = resolve_base();
            if let Err(e) = std::fs::create_dir_all(&dir) {
                log::error!(
                    "创建数据目录失败 {:?}: {}，回退到当前工作目录",
                    dir,
                    e
                );
                return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            }
            dir
        })
        .clone()
}

fn resolve_base() -> PathBuf {
    if let Ok(dir) = std::env::var(DATA_DIR_ENV) {
        let dir = dir.trim();
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }

    if let Ok(appdata) = std::env::var("APPDATA") {
        let appdata = appdata.trim();
        if !appdata.is_empty() {
            return PathBuf::from(appdata).join(APP_DIR_NAME);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join(APP_DIR_NAME);
        }
    }

    PathBuf::from(".").join(APP_DIR_NAME)
}

/// 主配置文件路径
pub fn config_file() -> PathBuf {
    data_dir().join("config.toml")
}

/// Cookie 文件路径
pub fn cookie_file() -> PathBuf {
    data_dir().join(crate::cookie::COOKIE_FILE_NAME)
}

/// 历史数据库路径
pub fn history_db() -> PathBuf {
    data_dir().join("history.db")
}

/// 把配置里的（可能为相对的）路径解析到数据目录下；绝对路径原样返回。
pub fn resolve(configured: &str) -> PathBuf {
    let path = Path::new(configured);
    if path.is_absolute() || configured.trim().is_empty() {
        if configured.trim().is_empty() {
            return data_dir();
        }
        path.to_path_buf()
    } else {
        data_dir().join(path)
    }
}

/// 日志文件路径（`configured` 默认为 `logs/bot.log`）
pub fn log_file(configured: &str) -> PathBuf {
    resolve(configured)
}

/// 一次迁移的结果（便于日志与测试断言）
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// 迁移成功的文件名
    pub files: Vec<String>,
    /// 是否迁移了旧日志目录
    pub logs_dir: bool,
}

/// 把旧版本遗留在 CWD / 程序目录下的数据搬进用户数据目录。
///
/// 只在目标文件**不存在**时迁移，绝不覆盖新目录里已有的数据。
/// 应在读取配置之前调用一次。
pub fn migrate_legacy_files() {
    let dest = data_dir();

    let mut sources: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        sources.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let parent = parent.to_path_buf();
            if !sources.contains(&parent) {
                sources.push(parent);
            }
        }
    }

    let report = migrate_from_sources(&dest, &sources);
    if !report.files.is_empty() || report.logs_dir {
        log::info!(
            "旧数据迁移完成: 迁移 {} 个文件{} -> {:?}",
            report.files.len(),
            if report.logs_dir { "，含日志目录" } else { "" },
            dest
        );
    }
}

/// 迁移的实际实现（与全局状态解耦，便于单元测试）。
pub fn migrate_from_sources(dest: &Path, sources: &[PathBuf]) -> MigrationReport {
    let mut report = MigrationReport::default();

    for src in sources {
        if src == dest {
            continue;
        }

        for name in DATA_FILES {
            let from = src.join(name);
            let to = dest.join(name);
            // 目标已存在时不覆盖：新目录里的数据永远优先
            if !from.is_file() || to.exists() {
                continue;
            }
            if move_file(&from, &to) {
                log::info!("已迁移旧数据文件: {:?} -> {:?}", from, to);
                report.files.push((*name).to_string());
            }
        }

        // 旧日志目录
        let old_logs = src.join(LOG_DIR_NAME);
        let new_logs = dest.join(LOG_DIR_NAME);
        if old_logs.is_dir() && !new_logs.exists() && move_dir(&old_logs, &new_logs) {
            log::info!("已迁移旧日志目录: {:?} -> {:?}", old_logs, new_logs);
            report.logs_dir = true;
        }
    }

    report
}

/// 移动文件；跨盘符时退化为「复制 + 删除」
fn move_file(from: &Path, to: &Path) -> bool {
    match std::fs::rename(from, to) {
        Ok(()) => true,
        Err(rename_err) => match std::fs::copy(from, to) {
            Ok(_) => {
                let _ = std::fs::remove_file(from);
                true
            }
            Err(copy_err) => {
                log::warn!(
                    "迁移 {:?} 失败: rename={} copy={}",
                    from,
                    rename_err,
                    copy_err
                );
                false
            }
        },
    }
}

fn move_dir(from: &Path, to: &Path) -> bool {
    if std::fs::rename(from, to).is_ok() {
        return true;
    }
    // 跨盘符：逐文件复制
    let mut ok = true;
    let entries = match std::fs::read_dir(from) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("读取旧日志目录 {:?} 失败: {}", from, e);
            return false;
        }
    };
    if let Err(e) = std::fs::create_dir_all(to) {
        log::warn!("创建日志目录 {:?} 失败: {}", to, e);
        return false;
    }
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let name = match path.file_name() {
                Some(n) => n,
                None => continue,
            };
            if !move_file(&path, &to.join(name)) {
                ok = false;
            }
        }
    }
    if ok {
        let _ = std::fs::remove_dir(from);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 为每个测试建一个独立的临时目录
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("bili_paths_tests").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建测试目录失败");
        dir
    }

    #[test]
    fn test_migrate_moves_legacy_files() {
        let src = scratch("move_src");
        let dest = scratch("move_dest");

        std::fs::write(src.join("config.toml"), "cookie = \"x\"").unwrap();
        std::fs::write(src.join("history.db"), "db").unwrap();
        std::fs::write(src.join("video_cache.json"), "[]").unwrap();

        let report = migrate_from_sources(&dest, std::slice::from_ref(&src));

        assert_eq!(report.files.len(), 3, "应迁移 3 个文件: {:?}", report.files);
        for name in ["config.toml", "history.db", "video_cache.json"] {
            assert!(dest.join(name).is_file(), "目标应存在 {}", name);
            assert!(!src.join(name).exists(), "源文件应已移走 {}", name);
        }
    }

    /// 关键安全语义：绝不覆盖新目录里已有的数据
    #[test]
    fn test_migrate_never_overwrites_existing_destination() {
        let src = scratch("keep_src");
        let dest = scratch("keep_dest");

        std::fs::write(src.join("config.toml"), "OLD").unwrap();
        std::fs::write(dest.join("config.toml"), "NEW").unwrap();

        let report = migrate_from_sources(&dest, std::slice::from_ref(&src));

        assert!(report.files.is_empty(), "不应迁移任何文件");
        assert_eq!(
            std::fs::read_to_string(dest.join("config.toml")).unwrap(),
            "NEW",
            "目标文件必须保持不变"
        );
        assert_eq!(
            std::fs::read_to_string(src.join("config.toml")).unwrap(),
            "OLD",
            "源文件应保持原样，不删除"
        );
    }

    /// 只迁移已知的运行时文件，绝不碰源码 / 仓库 / 其它无关文件
    #[test]
    fn test_migrate_ignores_unrelated_files() {
        let src = scratch("unrelated_src");
        let dest = scratch("unrelated_dest");

        // 模拟一个开发目录
        std::fs::create_dir_all(src.join("src")).unwrap();
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join("src/main.rs"), "fn main(){}").unwrap();
        std::fs::write(src.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        std::fs::write(src.join("package.json"), "{}").unwrap();
        std::fs::write(src.join("README.md"), "# hi").unwrap();
        std::fs::write(src.join("config.toml"), "real").unwrap();

        let report = migrate_from_sources(&dest, std::slice::from_ref(&src));

        assert_eq!(report.files, vec!["config.toml".to_string()]);
        assert!(src.join("src/main.rs").exists(), "源码不能被移动");
        assert!(src.join(".git/HEAD").exists(), ".git 不能被移动");
        assert!(src.join("package.json").exists());
        assert!(src.join("README.md").exists());
        assert!(!dest.join("src").exists());
        assert!(!dest.join("package.json").exists());
    }

    #[test]
    fn test_migrate_moves_logs_directory() {
        let src = scratch("logs_src");
        let dest = scratch("logs_dest");

        std::fs::create_dir_all(src.join("logs")).unwrap();
        std::fs::write(src.join("logs/bot.log"), "line").unwrap();

        let report = migrate_from_sources(&dest, std::slice::from_ref(&src));

        assert!(report.logs_dir, "应报告迁移了日志目录");
        assert!(dest.join("logs/bot.log").is_file(), "日志文件应被迁移");
        assert!(!src.join("logs").exists(), "旧日志目录应被移除");
    }

    #[test]
    fn test_migrate_skips_source_equal_to_dest() {
        let dir = scratch("self_src");
        std::fs::write(dir.join("config.toml"), "x").unwrap();

        let report = migrate_from_sources(&dir, std::slice::from_ref(&dir));

        assert!(report.files.is_empty(), "源与目标相同时不应做任何事");
        assert!(dir.join("config.toml").is_file());
    }

    #[test]
    fn test_migrate_handles_missing_source_dir() {
        let dest = scratch("missing_dest");
        let missing = dest.join("does-not-exist");
        let report = migrate_from_sources(&dest, &[missing]);
        assert_eq!(report, MigrationReport::default());
    }

    #[test]
    fn test_resolve_relative_goes_under_data_dir() {
        let resolved = resolve("video_cache.json");
        assert!(resolved.is_absolute(), "应解析为绝对路径: {:?}", resolved);
        assert!(resolved.ends_with("video_cache.json"));
        assert!(
            resolved.starts_with(data_dir()),
            "相对路径应挂在数据目录下: {:?}",
            resolved
        );
    }

    #[test]
    fn test_resolve_absolute_is_untouched() {
        let abs = if cfg!(windows) {
            r"C:\some\where\cache.json"
        } else {
            "/some/where/cache.json"
        };
        assert_eq!(resolve(abs), PathBuf::from(abs));
    }

    #[test]
    fn test_resolve_empty_falls_back_to_data_dir() {
        assert_eq!(resolve("   "), data_dir());
    }

    #[test]
    fn test_data_dir_is_absolute_and_created() {
        let dir = data_dir();
        assert!(dir.is_absolute(), "数据目录应为绝对路径: {:?}", dir);
        assert!(dir.exists(), "数据目录应已创建: {:?}", dir);
    }

    #[test]
    fn test_named_paths_live_under_data_dir() {
        let dir = data_dir();
        assert_eq!(config_file(), dir.join("config.toml"));
        assert_eq!(cookie_file(), dir.join("bilibili_cookie.json"));
        assert_eq!(history_db(), dir.join("history.db"));
        assert_eq!(log_file("logs/bot.log"), dir.join("logs").join("bot.log"));
    }
}
