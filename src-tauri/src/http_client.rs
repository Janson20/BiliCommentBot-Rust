/// HTTP 客户端工具
///
/// 提供 UA 轮换、APP 端通用参数等请求头管理辅助函数。
/// 主请求流程直接使用 reqwest::Client（见 bot.rs），此处仅提供公共辅助。
use rand::seq::SliceRandom;
use rand::thread_rng;

// ════════════════════════════════════════════════════════════════
//  UA 池 / APP 参数
// ════════════════════════════════════════════════════════════════

const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/121.0",
];

const APP_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 BiliDroid/8.43.0 (bbcallen@gmail.com) os/android model/android mobi_app/android build/8430300 channel/master innerVer/8430300 osVer/15 network/2",
    "Mozilla/5.0 BiliDroid/8.42.0 (bbcallen@gmail.com) os/android model/android mobi_app/android build/8420300 channel/master innerVer/8420300 osVer/14 network/2",
    "Mozilla/5.0 BiliDroid/8.43.0 (bbcallen@gmail.com) os/android model/android mobi_app/android_hd build/2001100 channel/master innerVer/2001100 osVer/15 network/2",
];

const APP_COMMON_PARAMS: &[(&str, &str)] = &[
    ("build", "2001100"),
    ("version", "2.0.1"),
    ("mobi_app", "android_hd"),
    ("platform", "android"),
    ("channel", "master"),
    ("c_locale", "zh_CN"),
    ("s_locale", "zh_CN"),
    ("statistics", "{\"appId\":5,\"platform\":3,\"version\":\"2.0.1\",\"abtest\":\"\"}"),
    ("qn", "80"),
];

// ════════════════════════════════════════════════════════════════
//  工具函数
// ════════════════════════════════════════════════════════════════

/// 随机选取一个 Web 端 User-Agent
pub fn random_web_ua() -> &'static str {
    let mut rng = thread_rng();
    USER_AGENTS.choose(&mut rng).copied().unwrap_or(USER_AGENTS[0])
}

/// 随机选取一个 BiliDroid（APP 端）User-Agent
pub fn random_app_ua() -> &'static str {
    let mut rng = thread_rng();
    APP_USER_AGENTS.choose(&mut rng).copied().unwrap_or(APP_USER_AGENTS[0])
}

/// 获取 APP 端通用查询参数（不含签名）
pub fn app_common_params() -> Vec<(&'static str, &'static str)> {
    APP_COMMON_PARAMS.to_vec()
}
