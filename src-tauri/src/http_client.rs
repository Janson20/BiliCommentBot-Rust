/// HTTP 客户端封装
///
/// 提供 UA 轮换、Referer 轮换、通用请求头管理等
use rand::seq::SliceRandom;
use rand::thread_rng;
use reqwest::{Client, ClientBuilder, Method, RequestBuilder};
use std::time::Duration;

// ════════════════════════════════════════════════════════════════
//  UA / Referer 池
// ════════════════════════════════════════════════════════════════

const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/121.0",
];

const REFERERS: &[&str] = &[
    "https://www.bilibili.com/",
    "https://search.bilibili.com/",
    "https://space.bilibili.com/",
];

const APP_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 BiliDroid/8.43.0 (bbcallen@gmail.com) os/android model/android mobi_app/android build/8430300 channel/master innerVer/8430300 osVer/15 network/2",
    "Mozilla/5.0 BiliDroid/8.42.0 (bbcallen@gmail.com) os/android model/android mobi_app/android build/8420300 channel/master innerVer/8420300 osVer/14 network/2",
    "Mozilla/5.0 BiliDroid/8.43.0 (bbcallen@gmail.com) os/android model/android_hd mobi_app/android_hd build/2001100 channel/master innerVer/2001100 osVer/15 network/2",
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

const APP_BASE_HEADERS: &[(&str, &str)] = &[
    ("env", "prod"),
    ("app-key", "android64"),
    ("x-bili-aurora-zone", "sh001"),
    ("bili-http-engine", "cronet"),
    ("Accept", "application/json"),
    ("Accept-Language", "zh-CN,zh;q=0.9"),
];

// ════════════════════════════════════════════════════════════════
//  HttpClient
// ════════════════════════════════════════════════════════════════

/// HTTP 客户端，封装 reqwest::Client + UA/Referer 轮换
#[derive(Debug, Clone)]
pub struct HttpClient {
    pub client: Client,
    /// 当前使用的 UA（用于日志等）
    current_ua: &'static str,
    /// 当前使用的 Referer
    current_referer: &'static str,
}

impl HttpClient {
    /// 创建新的 HTTP 客户端（Web 端模式）
    pub fn new_web(timeout_secs: u64) -> anyhow::Result<Self> {
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(5)
            .cookie_store(true)
            .gzip(true)
            .deflate(true)
            .brotli(true)
            .user_agent(random_web_ua())
            .build()?;

        Ok(Self {
            client,
            current_ua: random_web_ua(),
            current_referer: random_referer(),
        })
    }

    /// 构建一个基础请求（已含随机 UA 与 Referer）
    pub fn request(&self, method: Method, url: &str) -> RequestBuilder {
        let ua = random_web_ua();
        let referer = random_referer();
        self.client
            .request(method, url)
            .header("User-Agent", ua)
            .header("Referer", referer)
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Connection", "keep-alive")
    }

    /// 构建一个带 APP 端特征的请求（BiliDroid UA + APP headers）
    pub fn app_request(&self, method: Method, url: &str) -> RequestBuilder {
        let ua = random_app_ua();
        let mut builder = self
            .client
            .request(method, url)
            .header("User-Agent", ua);
        for (k, v) in APP_BASE_HEADERS {
            builder = builder.header(*k, *v);
        }
        builder
    }

    /// GET 请求
    pub fn get(&self, url: &str) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    /// POST 请求
    pub fn post(&self, url: &str) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    /// APP 端 GET
    pub fn app_get(&self, url: &str) -> RequestBuilder {
        self.app_request(Method::GET, url)
    }

    /// APP 端 POST
    pub fn app_post(&self, url: &str) -> RequestBuilder {
        self.app_request(Method::POST, url)
    }
}

// ════════════════════════════════════════════════════════════════
//  工具函数
// ════════════════════════════════════════════════════════════════

pub fn random_web_ua() -> &'static str {
    let mut rng = thread_rng();
    USER_AGENTS.choose(&mut rng).copied().unwrap_or(USER_AGENTS[0])
}

pub fn random_app_ua() -> &'static str {
    let mut rng = thread_rng();
    APP_USER_AGENTS.choose(&mut rng).copied().unwrap_or(APP_USER_AGENTS[0])
}

pub fn random_referer() -> &'static str {
    let mut rng = thread_rng();
    REFERERS.choose(&mut rng).copied().unwrap_or(REFERERS[0])
}

/// 获取 APP 端通用查询参数（不含签名）
pub fn app_common_params() -> Vec<(&'static str, &'static str)> {
    APP_COMMON_PARAMS.to_vec()
}
