/// 智能频率控制与请求重试
///
/// 对标 Python 版 `rate_limit_request` + `make_request_with_retry`：
/// - 自适应请求间隔（指数退避）
/// - B站频率限制错误码检测（-509 / -799 / 412 等）
/// - **按 `max_retries` / `retry_delay` 的真正重试**（含 `Retry-After` 支持）
///
/// 说明：早先的 Rust 端口只保留了配置字段却没有重试实现，导致一次瞬时 412/5xx
/// 就等于永久丢一条回复。现在由 [`RetryPolicy`] 统一提供重试。
use rand::Rng;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// B站频率限制相关错误码
const BILI_RATE_LIMIT_CODES: &[i64] = &[-509, -412, -799, 412, 509, 799, 10403];

/// 单次重试等待的上限，避免极端退避把机器人卡死
const MAX_BACKOFF_SECS: f64 = 120.0;

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

// ════════════════════════════════════════════════════════════════
//  重试
// ════════════════════════════════════════════════════════════════

/// 标记「值得重试」的错误，并可选携带服务端要求的 `Retry-After`
#[derive(Debug)]
pub struct RetryableError {
    pub message: String,
    pub retry_after: Option<u64>,
}

impl RetryableError {
    pub fn new(message: impl Into<String>, retry_after: Option<u64>) -> Self {
        Self {
            message: message.into(),
            retry_after,
        }
    }
}

impl std::fmt::Display for RetryableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.retry_after {
            Some(s) => write!(f, "{}（建议等待 {}s）", self.message, s),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for RetryableError {}

/// 构造一个可重试错误
pub fn retryable(message: impl Into<String>, retry_after: Option<u64>) -> anyhow::Error {
    anyhow::Error::new(RetryableError::new(message, retry_after))
}

/// 从响应头解析 `Retry-After`（仅支持秒数形式）
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
}

/// 重试策略快照（从 [`RateLimiter`] 取出后传入各网络模块）
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub retry_delay: u64,
    pub min_interval: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_delay: 5,
            min_interval: 2.0,
        }
    }
}

impl RetryPolicy {
    /// 总尝试次数（首次 + 重试）
    pub fn attempts(&self) -> u32 {
        self.max_retries.saturating_add(1)
    }

    /// 第 `attempt` 次重试前应等待的时长。
    ///
    /// 对标 Python：`max(retry_delay * 2^attempt, min_interval * (2 + attempt)) + jitter`，
    /// 服务端给了 `Retry-After` 时以它为准（并取两者较大值）。
    pub fn delay_before(&self, attempt: u32, retry_after: Option<u64>) -> Duration {
        let exp = self.retry_delay as f64 * 2f64.powi(attempt.min(10) as i32);
        let floor = self.min_interval * (2.0 + attempt as f64);
        let mut secs = exp.max(floor);
        if let Some(after) = retry_after {
            secs = secs.max(after as f64);
        }
        let jitter = rand::thread_rng().gen_range(0.0..2.0);
        Duration::from_secs_f64((secs + jitter).clamp(0.1, MAX_BACKOFF_SECS))
    }

    /// 按策略执行一个操作，遇到 [`RetryableError`] 时按指数退避重试。
    pub async fn run<T, F, Fut>(&self, label: &str, mut op: F) -> anyhow::Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<T>>,
    {
        let mut attempt = 0u32;
        loop {
            match op().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    let retry_after = err
                        .downcast_ref::<RetryableError>()
                        .and_then(|r| r.retry_after);

                    let is_retryable = err.downcast_ref::<RetryableError>().is_some();
                    if !is_retryable || attempt >= self.max_retries {
                        if is_retryable && self.max_retries > 0 {
                            log::warn!(
                                "{} 共尝试 {} 次后仍失败，放弃: {}",
                                label,
                                self.attempts(),
                                err
                            );
                        }
                        return Err(err);
                    }

                    let wait = self.delay_before(attempt, retry_after);
                    log::warn!(
                        "{} 失败，{}/{} 次重试前等待 {:.1}s：{}",
                        label,
                        attempt + 1,
                        self.max_retries,
                        wait.as_secs_f64(),
                        err
                    );
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
            }
        }
    }
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

    /// 取当前重试策略快照
    pub fn retry_policy(&self) -> RetryPolicy {
        let cfg = self.config.lock().unwrap();
        RetryPolicy {
            max_retries: cfg.max_retries,
            retry_delay: cfg.retry_delay,
            min_interval: cfg.min_interval,
        }
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
        let sleep_time = {
            let mut a = self.adaptive_interval.lock().unwrap();
            let v = self.current_interval();
            *a = v;
            let mut last = self.last_request.lock().unwrap();
            let elapsed = last.elapsed().as_secs_f64();
            let needed = if elapsed < v {
                let jitter = rand::thread_rng().gen_range(0.0..1.0);
                v - elapsed + jitter
            } else {
                0.0
            };
            *last = Instant::now();
            needed
        };
        if sleep_time > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(sleep_time)).await;
        }
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

/// 判断 B站 业务错误码是否属于频率限制
pub fn is_rate_limit_code(code: i64) -> bool {
    BILI_RATE_LIMIT_CODES.contains(&code)
}

/// 检测 B站响应体/状态码是否表示被限流（需要预先读取响应体文本）
pub fn is_bili_rate_limited_text(text: &str, status: reqwest::StatusCode) -> bool {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(code) = json["code"].as_i64() {
            if is_rate_limit_code(code) {
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

/// 服务器错误（5xx）通常也值得重试
pub fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_codes_include_positive_412() {
        // Python 版同时包含 ±412，早期端口漏掉了正值
        assert!(is_rate_limit_code(412));
        assert!(is_rate_limit_code(-412));
        assert!(is_rate_limit_code(-509));
        assert!(is_rate_limit_code(-799));
        assert!(!is_rate_limit_code(0));
        assert!(!is_rate_limit_code(-101));
    }

    #[test]
    fn test_is_bili_rate_limited_text() {
        assert!(is_bili_rate_limited_text(
            r#"{"code":-509,"message":"请求过于频繁，请稍后再试"}"#,
            reqwest::StatusCode::OK
        ));
        assert!(is_bili_rate_limited_text(
            r#"{"code":0}"#,
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(!is_bili_rate_limited_text(
            r#"{"code":0,"message":"0"}"#,
            reqwest::StatusCode::OK
        ));
        // 普通业务失败（如未登录）不应被当作限流
        assert!(!is_bili_rate_limited_text(
            r#"{"code":-101,"message":"账号未登录"}"#,
            reqwest::StatusCode::OK
        ));
    }

    #[test]
    fn test_retry_delay_is_exponential_and_capped() {
        let policy = RetryPolicy {
            max_retries: 3,
            retry_delay: 5,
            min_interval: 2.0,
        };
        let d0 = policy.delay_before(0, None).as_secs_f64();
        let d1 = policy.delay_before(1, None).as_secs_f64();
        let d2 = policy.delay_before(2, None).as_secs_f64();

        // 5*2^0=5, 5*2^1=10, 5*2^2=20（各含 0~2s 抖动）
        assert!((5.0..=7.0).contains(&d0), "d0={}", d0);
        assert!((10.0..=12.0).contains(&d1), "d1={}", d1);
        assert!((20.0..=22.0).contains(&d2), "d2={}", d2);
        // 单调递增
        assert!(d0 < d1 && d1 < d2);

        // 永不失控
        let huge = policy.delay_before(30, None).as_secs_f64();
        assert!(huge <= MAX_BACKOFF_SECS, "huge={}", huge);
    }

    #[test]
    fn test_retry_after_wins_when_larger() {
        let policy = RetryPolicy {
            max_retries: 3,
            retry_delay: 1,
            min_interval: 1.0,
        };
        let d = policy.delay_before(0, Some(30)).as_secs_f64();
        assert!(d >= 30.0, "应遵守 Retry-After，实际 {}", d);
    }

    #[test]
    fn test_min_interval_floor() {
        // 退避不能被配置成 0 间隔：min_interval 提供下限
        let policy = RetryPolicy {
            max_retries: 3,
            retry_delay: 0,
            min_interval: 3.0,
        };
        let d = policy.delay_before(0, None).as_secs_f64();
        assert!(d >= 6.0, "3*(2+0)=6 是下限，实际 {}", d);
    }

    #[test]
    fn test_attempts_counts_first_try() {
        assert_eq!(
            RetryPolicy {
                max_retries: 3,
                ..Default::default()
            }
            .attempts(),
            4
        );
        assert_eq!(
            RetryPolicy {
                max_retries: 0,
                ..Default::default()
            }
            .attempts(),
            1
        );
    }

    #[tokio::test]
    async fn test_run_retries_then_succeeds() {
        let policy = RetryPolicy {
            max_retries: 3,
            retry_delay: 0,
            min_interval: 0.01,
        };
        let attempts = std::sync::atomic::AtomicU32::new(0);
        let result: anyhow::Result<u32> = policy
            .run("测试", || async {
                let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 {
                    Err(retryable(format!("第 {} 次失败", n), None))
                } else {
                    Ok(n)
                }
            })
            .await;
        assert_eq!(result.unwrap(), 3);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_run_gives_up_after_max_retries() {
        let policy = RetryPolicy {
            max_retries: 2,
            retry_delay: 0,
            min_interval: 0.01,
        };
        let attempts = std::sync::atomic::AtomicU32::new(0);
        let result: anyhow::Result<()> = policy
            .run("测试", || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(retryable("总是失败", None))
            })
            .await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 3, "首次 + 2 次重试");
    }

    #[tokio::test]
    async fn test_run_does_not_retry_non_retryable() {
        let policy = RetryPolicy {
            max_retries: 3,
            retry_delay: 0,
            min_interval: 0.01,
        };
        let attempts = std::sync::atomic::AtomicU32::new(0);
        let result: anyhow::Result<()> = policy
            .run("测试", || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::anyhow!("业务拒绝，重试也没用"))
            })
            .await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1, "不可重试的错误只尝试一次");
    }
}
