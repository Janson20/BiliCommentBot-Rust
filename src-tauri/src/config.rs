/// 配置管理模块 —— 完全兼容 Python 版 BiliCommentBot 的 config.toml 格式
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

// ════════════════════════════════════════════════════════════════
//  配置数据结构
// ════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BilibiliConfig {
    #[serde(default)]
    pub cookie: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default = "default_check_interval")]
    pub check_interval: u64,
    #[serde(default = "default_true")]
    pub auto_refresh_cookie: bool,
    #[serde(default = "default_cookie_refresh_interval")]
    pub cookie_refresh_interval: u64,
    #[serde(default = "default_max_pages")]
    pub max_comment_pages: u32,
    #[serde(default = "default_max_pages")]
    pub max_video_pages: u32,
}

impl Default for BilibiliConfig {
    fn default() -> Self {
        Self {
            cookie: String::new(),
            refresh_token: String::new(),
            uid: String::new(),
            check_interval: default_check_interval(),
            auto_refresh_cookie: true,
            cookie_refresh_interval: default_cookie_refresh_interval(),
            max_comment_pages: default_max_pages(),
            max_video_pages: default_max_pages(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_min_request_interval")]
    pub min_request_interval: f64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_delay: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            min_request_interval: default_min_request_interval(),
            max_retries: default_max_retries(),
            retry_delay: default_retry_delay(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_cache_expire")]
    pub expire_time: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            expire_time: default_cache_expire(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoCacheConfig {
    #[serde(default = "default_video_cache_expire")]
    pub expire_time: u64,
    #[serde(default = "default_video_cache_file")]
    pub cache_file: String,
}

impl Default for VideoCacheConfig {
    fn default() -> Self {
        Self {
            expire_time: default_video_cache_expire(),
            cache_file: default_video_cache_file(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepseekConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_deepseek_base_url")]
    pub base_url: String,
    #[serde(default = "default_deepseek_model")]
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
}

impl Default for DeepseekConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: default_deepseek_base_url(),
            model: default_deepseek_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            system_prompt: default_system_prompt(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_base_url")]
    pub base_url: String,
    #[serde(default = "default_ollama_model")]
    pub model: String,
    #[serde(default = "default_ollama_timeout")]
    pub timeout_secs: u64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: default_ollama_base_url(),
            model: default_ollama_model(),
            timeout_secs: default_ollama_timeout(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub prefix: String,
    #[serde(default = "default_true")]
    pub only_new: bool,
    #[serde(default = "default_max_process")]
    pub max_process: u32,
    #[serde(default = "default_reply_delay")]
    pub reply_delay: u64,
    #[serde(default)]
    pub like_enabled: bool,
    #[serde(default)]
    pub context_comments_count: u32,
    #[serde(default)]
    pub only_bvid: String,
    #[serde(default)]
    pub like_user_video_enabled: bool,
    #[serde(default)]
    pub like_user_video_only_followers: bool,
    #[serde(default = "default_true")]
    pub chained_reply_enabled: bool,
    #[serde(default = "default_max_reply_depth")]
    pub max_reply_depth: u32,
}

impl Default for ReplyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prefix: String::new(),
            only_new: true,
            max_process: default_max_process(),
            reply_delay: default_reply_delay(),
            like_enabled: false,
            context_comments_count: 0,
            only_bvid: String::new(),
            like_user_video_enabled: false,
            like_user_video_only_followers: false,
            chained_reply_enabled: true,
            max_reply_depth: default_max_reply_depth(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_file")]
    pub file: String,
    #[serde(default = "default_true")]
    pub console: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file: default_log_file(),
            console: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub password: String,
}

/// AI 提供商选择
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    Deepseek,
    Ollama,
}

impl Default for AiProvider {
    fn default() -> Self {
        AiProvider::Deepseek
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub provider: AiProvider,
}

/// 顶层配置 —— 完全兼容 Python 版 config.toml 的 section 结构
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawConfig {
    #[serde(default)]
    pub bilibili: BilibiliConfig,
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub video_cache: VideoCacheConfig,
    #[serde(default)]
    pub deepseek: DeepseekConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub reply: ReplyConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    /// AI 提供商选择（新增字段，Python 版无此字段，兼容处理）
    #[serde(default)]
    pub ai: AiConfig,
}

// ════════════════════════════════════════════════════════════════
//  默认值函数
// ════════════════════════════════════════════════════════════════

fn default_true() -> bool { true }
fn default_check_interval() -> u64 { 60 }
fn default_cookie_refresh_interval() -> u64 { 30 }
fn default_max_pages() -> u32 { 10 }
fn default_min_request_interval() -> f64 { 2.0 }
fn default_max_retries() -> u32 { 3 }
fn default_retry_delay() -> u64 { 5 }
fn default_cache_expire() -> u64 { 300 }
fn default_video_cache_expire() -> u64 { 43200 }
fn default_video_cache_file() -> String { "video_cache.json".into() }
fn default_deepseek_base_url() -> String { "https://api.deepseek.com/v1".into() }
fn default_deepseek_model() -> String { "deepseek-chat".into() }
fn default_max_tokens() -> u32 { 200 }
fn default_temperature() -> f64 { 0.7 }
fn default_system_prompt() -> String {
    "你是一个友善的B站UP主，请对评论做出自然、友好的回复。回复要简洁明了，控制在100字以内。".into()
}
fn default_ollama_base_url() -> String { "http://127.0.0.1:11434".into() }
fn default_ollama_model() -> String { "qwen2.5:7b".into() }
fn default_ollama_timeout() -> u64 { 60 }
fn default_max_process() -> u32 { 10 }
fn default_reply_delay() -> u64 { 2 }
fn default_max_reply_depth() -> u32 { 3 }
fn default_log_level() -> String { "INFO".into() }
fn default_log_file() -> String { "logs/bot.log".into() }

// ════════════════════════════════════════════════════════════════
//  运行时配置管理器 (AppConfig)
// ════════════════════════════════════════════════════════════════

pub struct AppConfig {
    inner: RwLock<ConfigState>,
}

struct ConfigState {
    config: RawConfig,
    file_path: PathBuf,
}

const CONFIG_FILE_NAME: &str = "config.toml";

impl AppConfig {
    /// 新建配置管理器，自动尝试从当前目录加载 config.toml
    pub fn new() -> Self {
        let file_path = Self::default_config_path();
        let config = Self::load_or_default(&file_path);
        Self {
            inner: RwLock::new(ConfigState { config, file_path }),
        }
    }

    /// 获取当前配置的只读副本
    pub fn get(&self) -> RawConfig {
        self.inner.read().unwrap().config.clone()
    }

    /// 更新配置并保存到文件
    pub fn save(&self, new_config: RawConfig) -> Result<()> {
        let state = self.inner.read().unwrap();
        let content = toml::to_string_pretty(&new_config).context("序列化配置失败")?;
        // 确保目录存在
        if let Some(parent) = state.file_path.parent() {
            fs::create_dir_all(parent).context("创建配置目录失败")?;
        }
        fs::write(&state.file_path, &content).context("写入配置文件失败")?;
        drop(state);
        // 更新内存中的配置
        self.inner.write().unwrap().config = new_config;
        Ok(())
    }

    /// 重新从磁盘加载配置
    pub fn reload(&self) -> Result<RawConfig> {
        let state = self.inner.read().unwrap();
        let config = Self::load_or_default(&state.file_path);
        drop(state);
        self.inner.write().unwrap().config = config.clone();
        Ok(config)
    }

    /// 获取配置文件路径
    pub fn file_path(&self) -> PathBuf {
        self.inner.read().unwrap().file_path.clone()
    }

    // ── 内部方法 ──

    fn default_config_path() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(CONFIG_FILE_NAME)
    }

    fn load_or_default(path: &PathBuf) -> RawConfig {
        if !path.exists() {
            log::info!("配置文件不存在，将使用默认配置: {:?}", path);
            return RawConfig::default();
        }
        match fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<RawConfig>(&content) {
                Ok(cfg) => {
                    log::info!("成功加载配置文件: {:?}", path);
                    cfg
                }
                Err(e) => {
                    log::error!("配置文件解析失败: {}，将使用默认配置", e);
                    RawConfig::default()
                }
            },
            Err(e) => {
                log::error!("读取配置文件失败: {}，将使用默认配置", e);
                RawConfig::default()
            }
        }
    }
}

impl std::fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppConfig")
            .field("file_path", &self.inner.read().unwrap().file_path)
            .finish_non_exhaustive()
    }
}

// ════════════════════════════════════════════════════════════════
//  测试
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = RawConfig::default();
        assert_eq!(cfg.bilibili.check_interval, 60);
        assert_eq!(cfg.deepseek.base_url, "https://api.deepseek.com/v1");
        assert_eq!(cfg.rate_limit.max_retries, 3);
        assert_eq!(cfg.reply.max_process, 10);
    }

    #[test]
    fn test_config_roundtrip() {
        let cfg = RawConfig::default();
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: RawConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.bilibili.check_interval, cfg.bilibili.check_interval);
        assert_eq!(parsed.deepseek.model, cfg.deepseek.model);
    }
}
