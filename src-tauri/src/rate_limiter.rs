/// 智能频率控制
///
/// 对标 Python 版 rate_limit_request + make_request_with_retry：
/// - 指数退避 (base * 2^failures)
/// - 自适应间隔
/// - B站频率限制错误码检测 (-509, -799, 412 等)
/// 智能频率控制
use rand::Rng;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// B站频率限制相关错误码
const BILI_RATE_LIMIT_CODES: &[i64] = &[-509, -412, -799, 509, 799, 10403];

#[derive(Debug)]
struct LimiterConfig {
    min_interval: f64,
    max_retries: u32,
    retry_delay: u64,
}

#[derive(Debug)]
pub struct RateLimiter {
    config: Mutex<LimiterConfig>,
    last_request: Mutex<Instant>,
    consecutive_failures: AtomicU32,
    adaptive_interval: Mutex<f64>,
}

impl RateLimiter {
    pub fn new(min_interval: f64, max_retries: u32, retry_delay: u64) -> Self {
        Self {
            config: Mutex::new(LimiterConfig {
                min_interval,
                max_retries,
                retry_delay,
            }),
            last_request: Mutex::new(Instant::now()),
            consecutive_failures: AtomicU32::new(0),
            adaptive_interval: Mutex::new(min_interval),
        }
    }

    /// 运行时重新配置（热更新生效）
    pub fn reconfigure(&self, min_interval: f64, max_retries: u32, retry_delay: u64) {
        let mut cfg = self.config.lock().unwrap();
        cfg.min_interval = min_interval;
        cfg.max_retries = max_retries;
        cfg.retry_delay = retry_delay;
        // 重置自适应间隔
        *self.adaptive_interval.lock().unwrap() = min_interval;
    }

    /// 计算当前应使用的请求间隔（含自适应退避）
    pub fn current_interval(&self) -> f64 {
        let failures = self.consecutive_failures.load(Ordering::Relaxed);
        if failures > 0 {
            let cfg = self.config.lock().unwrap();
            let exponent = failures.min(10);
            (cfg.min_interval * (2u64.pow(exponent) as f64)).min(cfg.min_interval * 10.0)
        } else {
            self.config.lock().unwrap().min_interval
        }
    }

    /// 等待直到可以发送请求（异步非阻塞）
    pub async fn wait(&self) {
        let interval = {
            let mut a = self.adaptive_interval.lock().unwrap();
            let v = self.current_interval();
            *a = v;
            v
        };
        let mut last = self.last_request.lock().unwrap();
        let elapsed = last.elapsed().as_secs_f64();
        if elapsed < interval {
            let jitter = rand::thread_rng().gen_range(0.0..1.0);
            let sleep_time = interval - elapsed + jitter;
            tokio::time::sleep(Duration::from_secs_f64(sleep_time)).await;
        }
        *last = Instant::now();
    }

    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn failure_count(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }
}

/// 检测 B站响应体中的频率限制错误码（需要预先读取响应体文本）
pub fn is_bili_rate_limited_text(text: &str, status: reqwest::StatusCode) -> bool {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(code) = json["code"].as_i64() {
            if BILI_RATE_LIMIT_CODES.contains(&code) {
                return true;
            }
        }
        if let Some(msg) = json["message"].as_str() {
            if msg.contains("过于频繁")
                || msg.contains("请求过于频繁")
                || msg.contains("访问被拒绝")
            {
                return true;
            }
        }
    }
    false
}
