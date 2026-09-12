//! 开机自启管理
//!
//! Windows 下通过写入**当前用户**的注册表 Run 项实现（无需管理员权限）：
//!
//! ```text
//! HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
//!     BiliCommentBot-RS = "D:\...\BiliCommentBot-RS.exe" --minimized
//! ```
//!
//! 命令行末尾带 [`MINIMIZED_ARG`]，开机后程序静默启动并直接进入系统托盘，
//! 不会弹出主窗口。该注册表项同时会出现在「任务管理器 → 启动」列表中，
//! 用户也可以在那里自行禁用。

use std::path::PathBuf;

/// 自启命令行携带的参数：启动后不显示主窗口（最小化到托盘）
pub const MINIMIZED_ARG: &str = "--minimized";

/// 注册表 Run 项中的值名
pub const VALUE_NAME: &str = "BiliCommentBot-RS";

/// 当前可执行文件路径
pub fn exe_path() -> anyhow::Result<PathBuf> {
    std::env::current_exe().map_err(|e| anyhow::anyhow!("获取程序路径失败: {}", e))
}

/// 开机自启使用的完整命令行（路径含空格，因此加引号）
pub fn launch_command() -> anyhow::Result<String> {
    Ok(format!("\"{}\" {}", exe_path()?.display(), MINIMIZED_ARG))
}

/// 本次是否由开机自启拉起（即命令行带 `--minimized`）
pub fn started_minimized() -> bool {
    std::env::args()
        .skip(1)
        .any(|arg| arg.eq_ignore_ascii_case(MINIMIZED_ARG))
}

/// 当前是否已开启开机自启（以注册表为准）
pub fn is_enabled() -> bool {
    platform::is_enabled()
}

/// 开启开机自启；重复调用会覆盖为当前程序路径
pub fn enable() -> anyhow::Result<()> {
    platform::enable()
}

/// 关闭开机自启（注册表项不存在时视为成功）
pub fn disable() -> anyhow::Result<()> {
    platform::disable()
}

/// 按需开启/关闭
pub fn set(enabled: bool) -> anyhow::Result<()> {
    if enabled {
        enable()
    } else {
        disable()
    }
}

/// 配置里记为已开启时，每次启动都刷新一遍注册表项，
/// 保证程序更新或移动位置后自启命令仍指向正确的 exe
pub fn sync_if_enabled(enabled: bool) {
    if !enabled {
        return;
    }
    match enable() {
        Ok(()) => log::info!("已刷新开机自启注册表项: {:?}", launch_command()),
        Err(e) => log::warn!("刷新开机自启注册表项失败: {}", e),
    }
}

// ════════════════════════════════════════════════════════════════
//  Windows 实现
// ════════════════════════════════════════════════════════════════

#[cfg(windows)]
mod platform {
    use super::{launch_command, VALUE_NAME};
    use anyhow::{Context, Result};
    use std::io::ErrorKind;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn is_enabled() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey(RUN_KEY)
            .and_then(|key| key.get_value::<String, _>(VALUE_NAME))
            .map(|cmd| !cmd.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn enable() -> Result<()> {
        let command = launch_command()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(RUN_KEY)
            .context("打开注册表 Run 项失败")?;
        key.set_value(VALUE_NAME, &command)
            .context("写入注册表 Run 项失败")?;
        log::info!("已开启开机自启: {}", command);
        Ok(())
    }

    pub fn disable() -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
            Ok(key) => key,
            // Run 项都不存在，说明本来就没开启
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(anyhow::anyhow!("打开注册表 Run 项失败: {}", e)),
        };
        match key.delete_value(VALUE_NAME) {
            Ok(()) => {
                log::info!("已关闭开机自启");
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow::anyhow!("删除注册表 Run 项失败: {}", e)),
        }
    }
}

// ════════════════════════════════════════════════════════════════
//  非 Windows 平台（占位实现，保证可编译）
// ════════════════════════════════════════════════════════════════

#[cfg(not(windows))]
mod platform {
    use anyhow::Result;

    pub fn is_enabled() -> bool {
        false
    }

    pub fn enable() -> Result<()> {
        anyhow::bail!("开机自启目前仅支持 Windows")
    }

    pub fn disable() -> Result<()> {
        anyhow::bail!("开机自启目前仅支持 Windows")
    }
}

// ════════════════════════════════════════════════════════════════
//  测试
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launch_command_quotes_exe_and_passes_flag() {
        let cmd = launch_command().expect("应能取到当前进程路径");
        assert!(cmd.starts_with('"'), "exe 路径应被引号包裹: {}", cmd);
        assert!(
            cmd.ends_with(MINIMIZED_ARG),
            "自启命令应带 {} 参数: {}",
            MINIMIZED_ARG,
            cmd
        );
    }

    #[test]
    fn test_exe_path_exists() {
        let exe = exe_path().expect("应能取到当前进程路径");
        assert!(exe.is_file(), "当前进程路径应指向真实文件: {:?}", exe);
    }

    /// 真实注册表读写往返测试（会临时写入再删除 Run 项，结束后恢复原状态）
    ///
    /// 默认被 `#[ignore]` 跳过，避免污染开发者机器的自启设置；
    /// 需要验证注册表逻辑时手动执行：
    ///
    /// ```text
    /// cargo test -- --ignored test_autostart_registry_roundtrip --nocapture
    /// ```
    #[test]
    #[ignore = "会读写真实注册表，需手动执行"]
    #[cfg(windows)]
    fn test_autostart_registry_roundtrip() {
        let original = is_enabled();
        if original {
            disable().expect("测试前置：关闭自启失败");
        }

        assert!(!is_enabled(), "前置条件：Run 项应不存在");

        enable().expect("开启开机自启失败");
        assert!(is_enabled(), "写入注册表后应报告已开启");

        let command = launch_command().unwrap();
        assert!(command.contains(MINIMIZED_ARG));

        disable().expect("关闭开机自启失败");
        assert!(!is_enabled(), "删除注册表后应报告已关闭");
        // 重复删除应幂等
        disable().expect("重复关闭开机自启应成功");

        if original {
            enable().expect("测试后恢复原自启状态失败");
        }
    }

    /// 非 Windows 平台应明确拒绝而不是静默成功
    #[test]
    #[cfg(not(windows))]
    fn test_autostart_unsupported() {
        assert!(!is_enabled());
        assert!(enable().is_err());
        assert!(disable().is_err());
    }
}
