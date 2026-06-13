/// B站 Cookie 管理器
///
/// 功能对标 Python 版 BilibiliCookieManager：
/// - Cookie 字符串解析与存储
/// - CSRF token (bili_jct) 提取
/// - Cookie 有效性检查 & 验证
/// - 自动刷新 (含 refresh_csrf 提取)
/// - 本地持久化 (JSON)
/// - 扫码登录 (生成二维码 + 轮询)

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use qrcode::QrCode;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ════════════════════════════════════════════════════════════════
//  常量
// ════════════════════════════════════════════════════════════════

const HEADERS_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const GENERATE_URL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
const POLL_URL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";
const COOKIE_INFO_URL: &str = "https://passport.bilibili.com/x/passport-login/web/cookie/info";
const REFRESH_URL: &str = "https://passport.bilibili.com/x/passport-login/web/cookie/refresh";
const VERIFY_URL: &str = "https://api.bilibili.com/x/space/myinfo";
const CORRESPOND_URL: &str = "https://www.bilibili.com/correspond/1/{}";

pub const DEFAULT_COOKIE_FILE: &str = "bilibili_cookie.json";

// ════════════════════════════════════════════════════════════════
//  数据模型
// ════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieData {
    pub cookie: HashMap<String, String>,
    pub refresh_token: String,
    pub timestamp: u64,
}

/// 扫码轮询结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrPollResult {
    /// 状态码（参考 B 站 API：0=成功 86039=未扫码 86038=已过期 86101=已扫码待确认）
    pub code: i32,
    pub message: String,
    pub cookies: HashMap<String, String>,
    pub refresh_token: Option<String>,
}

/// 扫码登录第一步：生成二维码
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrGenerateResult {
    pub url: String,
    pub qrcode_key: String,
    /// PNG 二维码图片的 Base64 编码
    pub qrcode_base64: String,
}

/// Cookie 状态检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieStatusResult {
    pub need_refresh: bool,
    pub message: String,
}

/// Cookie 验证结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieVerifyResult {
    pub valid: bool,
    pub message: String,
    pub uid: Option<String>,
    pub uname: Option<String>,
}

// ════════════════════════════════════════════════════════════════
//  CookieManager
// ════════════════════════════════════════════════════════════════

pub struct CookieManager {
    client: reqwest::Client,
    pub cookies: HashMap<String, String>,
    pub refresh_token: String,
    pub csrf_token: Option<String>,
}

impl CookieManager {
    /// 使用 reqwest Client 创建
    pub fn from_client(client: reqwest::Client) -> Self {
        Self {
            client,
            cookies: HashMap::new(),
            refresh_token: String::new(),
            csrf_token: None,
        }
    }

    /// 使用传入的 reqwest::Client + cookie/refresh_token 构建
    pub async fn new(
        client: reqwest::Client,
        cookie_str: &str,
        refresh_token: &str,
    ) -> Self {
        let mut mgr = Self {
            client,
            cookies: HashMap::new(),
            refresh_token: refresh_token.to_string(),
            csrf_token: None,
        };
        if !cookie_str.is_empty() {
            mgr.set_cookie_from_str(cookie_str);
        }
        mgr.csrf_token = mgr.get_csrf_from_cookie();
        mgr
    }

    // ════════════════════════════════════════════════════════════
    //  Cookie 解析
    // ════════════════════════════════════════════════════════════

    /// 从 key=value; key=value 格式字符串解析 Cookie
    pub fn set_cookie_from_str(&mut self, cookie_str: &str) {
        for part in cookie_str.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((key, value)) = part.split_once('=') {
                self.cookies
                    .insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }

    /// 将当前 Cookie 转为 key=value; key=value 格式字符串
    pub fn get_cookie_str(&self) -> String {
        self.cookies
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// 将 Cookie 转为 reqwest HeaderMap
    pub fn to_cookie_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&self.get_cookie_str()) {
            headers.insert("Cookie", val);
        }
        headers
    }

    /// 从 Cookie 中提取 bili_jct (CSRF Token)
    pub fn get_csrf_from_cookie(&self) -> Option<String> {
        self.cookies.get("bili_jct").cloned()
    }

    // ════════════════════════════════════════════════════════════
    //  扫码登录
    // ════════════════════════════════════════════════════════════

    /// 获取扫码登录二维码（返回 URL + key + Base64 PNG 图片）
    pub async fn generate_qrcode(&self) -> Result<QrGenerateResult> {
        let resp = self
            .client
            .get(GENERATE_URL)
            .header("User-Agent", HEADERS_UA)
            .header("Referer", "https://www.bilibili.com")
            .send()
            .await
            .context("获取二维码请求失败")?;

        let json: serde_json::Value = resp.json().await.context("解析二维码响应失败")?;
        let code = json["code"].as_i64().unwrap_or(-1);
        if code != 0 {
            return Err(anyhow!(
                "获取二维码失败: {}",
                json["message"].as_str().unwrap_or("未知错误")
            ));
        }

        let data = &json["data"];
        let url = data["url"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let qrcode_key = data["qrcode_key"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // 生成二维码 PNG → Base64
        let qrcode_base64 = generate_qr_png_base64(&url)?;

        Ok(QrGenerateResult {
            url,
            qrcode_key,
            qrcode_base64,
        })
    }

    /// 轮询扫码状态，返回登录后的 Cookie
    pub async fn poll_qr_login(&self, qrcode_key: &str) -> Result<QrPollResult> {
        let _ts = current_timestamp();
        let params = [
            ("qrcode_key", qrcode_key.to_string()),
            ("source", "main-fe-header".to_string()),
        ];

        let resp = self
            .client
            .get(POLL_URL)
            .query(&params)
            .header("User-Agent", HEADERS_UA)
            .header("Referer", "https://www.bilibili.com")
            .send()
            .await
            .context("轮询扫码状态失败")?;

        let json: serde_json::Value = resp.json().await.context("解析轮询响应失败")?;
        let data = &json["data"];
        let poll_code = data["code"].as_i64().unwrap_or(-1) as i32;
        let message = data["message"]
            .as_str()
            .unwrap_or("未知状态")
            .to_string();

        let mut cookies = HashMap::new();
        let mut refresh_token: Option<String> = None;

        // 扫码成功 (code==0) 或 已确认 (code==0 时 url 中含有 cookie 信息)
        if poll_code == 0 {
            // B站 扫码成功后响应中可能直接带了 set-cookie 头
            // 但轮询接口不会直接给 cookie，需要通过后续请求获取
            // 此处使用 refresh_token 字段
            refresh_token = data["refresh_token"].as_str().map(|s| s.to_string());

            // 尝试从 B站 返回的 cookie 字段获取
            if let Some(cookie_map) = data["cookie_info"].as_object() {
                for (k, v) in cookie_map {
                    if let Some(vs) = v.as_str() {
                        cookies.insert(k.clone(), vs.to_string());
                    }
                }
            }
        }

        // 非成功状态但已扫码 (86090 = 已扫码未确认, 86101 = 已扫码待确认)
        Ok(QrPollResult {
            code: poll_code,
            message,
            cookies,
            refresh_token,
        })
    }

    // ════════════════════════════════════════════════════════════
    //  Cookie 检查与验证
    // ════════════════════════════════════════════════════════════

    /// 检查 Cookie 是否需要刷新
    pub async fn check_cookie_status(&self) -> Result<CookieStatusResult> {
        let resp = self
            .client
            .get(COOKIE_INFO_URL)
            .headers(self.to_cookie_headers())
            .header("User-Agent", HEADERS_UA)
            .header("Referer", "https://www.bilibili.com")
            .send()
            .await
            .context("检查Cookie状态失败")?;

        let json: serde_json::Value = resp.json().await?;
        if json["code"].as_i64() == Some(0) {
            let need_refresh = json["data"]["refresh"]
                .as_bool()
                .unwrap_or(false);
            Ok(CookieStatusResult {
                need_refresh,
                message: "OK".to_string(),
            })
        } else {
            Ok(CookieStatusResult {
                need_refresh: false,
                message: json["message"]
                    .as_str()
                    .unwrap_or("未知错误")
                    .to_string(),
            })
        }
    }

    /// 验证 Cookie 是否有效（通过 /x/space/myinfo 获取用户信息）
    pub async fn verify_cookie(&self) -> CookieVerifyResult {
        let resp = match self
            .client
            .get(VERIFY_URL)
            .headers(self.to_cookie_headers())
            .header("User-Agent", HEADERS_UA)
            .header("Referer", "https://www.bilibili.com")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return CookieVerifyResult {
                    valid: false,
                    message: format!("请求失败: {}", e),
                    uid: None,
                    uname: None,
                }
            }
        };

        let json: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                return CookieVerifyResult {
                    valid: false,
                    message: format!("解析响应失败: {}", e),
                    uid: None,
                    uname: None,
                }
            }
        };

        if json["code"].as_i64() == Some(0) {
            let data = &json["data"];
            CookieVerifyResult {
                valid: true,
                message: "Cookie有效".to_string(),
                uid: data["mid"].as_u64().map(|v| v.to_string()),
                uname: data["name"].as_str().map(|s| s.to_string()),
            }
        } else {
            CookieVerifyResult {
                valid: false,
                message: json["message"]
                    .as_str()
                    .unwrap_or("验证失败")
                    .to_string(),
                uid: None,
                uname: None,
            }
        }
    }

    // ════════════════════════════════════════════════════════════
    //  Cookie 刷新
    // ════════════════════════════════════════════════════════════

    /// 获取 refresh_csrf（从 B站 correspond 接口 HTML 中提取）
    pub async fn get_refresh_csrf(&self) -> Result<String> {
        let timestamp = current_timestamp() as u64;
        let md5_hash = format!("{:x}", md5::compute(timestamp.to_string()));
        let correspond_path = format!(
            "/apis/redirect/login?from=bilibili.com&timestamp={}&md5={}",
            timestamp, md5_hash
        );
        let encoded_path =
            urlencoding::encode(&correspond_path);
        let url = CORRESPOND_URL.replace("{}", &encoded_path);

        let resp = self
            .client
            .get(&url)
            .headers(self.to_cookie_headers())
            .header("User-Agent", HEADERS_UA)
            .header("Referer", "https://www.bilibili.com")
            .send()
            .await
            .context("获取refresh_csrf请求失败")?;

        let html = resp.text().await.context("读取refresh_csrf响应失败")?;

        // 用正则提取 refresh_csrf
        let patterns = [
            r#""refresh_csrf"\s*:\s*"([^"]+)""#,
            r"refresh_csrf\s*=\s*'([^']+)'",
            r#"refresh_csrf\s*=\s*"([^"]+)""#,
        ];

        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(&html) {
                    if let Some(m) = caps.get(1) {
                        return Ok(m.as_str().to_string());
                    }
                }
            }
        }

        // Fallback: 从 cookie 中获取
        self.cookies
            .get("refresh_csrf")
            .cloned()
            .ok_or_else(|| anyhow!("未找到refresh_csrf"))
    }

    /// 刷新 Cookie（通过 B站 passport 接口）
    ///
    /// 返回 (成功, 消息, 新的 refresh_token)
    pub async fn refresh_cookie(&mut self) -> Result<(bool, String, Option<String>)> {
        let refresh_token = self.refresh_token.clone();
        if refresh_token.is_empty() {
            return Ok((false, "refresh_token不存在".to_string(), None));
        }

        let refresh_csrf = self.get_refresh_csrf().await?;
        let csrf_token = self
            .get_csrf_from_cookie()
            .ok_or_else(|| anyhow!("获取CSRF token失败"))?;

        let params = [
            ("csrf", csrf_token.as_str()),
            ("refresh_csrf", &refresh_csrf),
            ("refresh_token", &refresh_token),
            ("source", "main_web"),
        ];

        let resp = self
            .client
            .post(REFRESH_URL)
            .form(&params)
            .headers(self.to_cookie_headers())
            .header("User-Agent", HEADERS_UA)
            .header("Referer", "https://www.bilibili.com")
            .header("Origin", "https://www.bilibili.com")
            .send()
            .await
            .context("刷新Cookie请求失败")?;

        // 提取响应中的 Set-Cookie 头
        for cookie_header in resp.headers().get_all("set-cookie") {
            if let Ok(cookie_str) = cookie_header.to_str() {
                if let Some((key, value)) = cookie_str.split_once('=') {
                    // 去掉路径和过期信息
                    let val = value.split(';').next().unwrap_or("");
                    self.cookies
                        .insert(key.to_string(), val.to_string());
                }
            }
        }

        let json: serde_json::Value = resp.json().await?;
        if json["code"].as_i64() == Some(0) {
            let data = &json["data"];
            let new_refresh_token = data["refresh_token"]
                .as_str()
                .map(|s| s.to_string());
            if let Some(ref token) = new_refresh_token {
                self.refresh_token = token.clone();
            }
            self.csrf_token = self.get_csrf_from_cookie();
            Ok((true, "刷新成功".to_string(), new_refresh_token))
        } else {
            Ok((
                false,
                json["message"]
                    .as_str()
                    .unwrap_or("刷新失败")
                    .to_string(),
                None,
            ))
        }
    }

    /// 自动检查并刷新 Cookie（如果需要）
    pub async fn auto_refresh_if_needed(&mut self) -> Result<(bool, String)> {
        let status = self.check_cookie_status().await?;
        if status.need_refresh {
            match self.refresh_cookie().await {
                Ok((true, msg, _)) => Ok((true, msg)),
                Ok((false, msg, _)) => Ok((false, format!("Cookie刷新失败: {}", msg))),
                Err(e) => Ok((false, format!("Cookie刷新异常: {}", e))),
            }
        } else {
            Ok((false, "Cookie状态正常，无需刷新".to_string()))
        }
    }

    // ════════════════════════════════════════════════════════════
    //  持久化
    // ════════════════════════════════════════════════════════════

    /// 保存 Cookie 到 JSON 文件
    pub fn save_to_file(&self, path: &PathBuf) -> Result<()> {
        let data = CookieData {
            cookie: self.cookies.clone(),
            refresh_token: self.refresh_token.clone(),
            timestamp: current_timestamp(),
        };
        let content = serde_json::to_string_pretty(&data).context("序列化Cookie数据失败")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("创建Cookie文件目录失败")?;
        }
        fs::write(path, &content).context("写入Cookie文件失败")?;
        log::info!("Cookie已保存到: {:?}", path);
        Ok(())
    }

    /// 从 JSON 文件加载 Cookie
    pub fn load_from_file(&mut self, path: &PathBuf) -> Result<()> {
        if !path.exists() {
            return Err(anyhow!("Cookie文件不存在: {:?}", path));
        }
        let content = fs::read_to_string(path).context("读取Cookie文件失败")?;
        let data: CookieData =
            serde_json::from_str(&content).context("解析Cookie文件失败")?;

        self.cookies = data.cookie;
        self.refresh_token = data.refresh_token;
        self.csrf_token = self.get_csrf_from_cookie();
        log::info!("成功从文件加载Cookie: {:?}", path);
        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════
//  工具函数
// ════════════════════════════════════════════════════════════════

/// 生成二维码 PNG 并转为 Base64 字符串（data:image/png;base64,...）
fn generate_qr_png_base64(url: &str) -> Result<String> {
    use image::ImageEncoder;
    let code = QrCode::new(url).map_err(|e| anyhow!("生成二维码失败: {}", e))?;
    let img = code.render::<image::Luma<u8>>().build();

    // 编码为 PNG 字节
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::L8,
        )
        .map_err(|e| anyhow!("PNG编码失败: {}", e))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Ok(format!("data:image/png;base64,{}", b64))
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
