//! 传输速率限制器
//!
//! 使用令牌桶算法实现用户上传/下载速度限制

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const BUCKET_CAPACITY: u64 = 64 * 1024;
const REFILL_INTERVAL_MS: u64 = 10;

/// 高性能限流器：使用令牌桶算法 + 后台补充
pub struct RateLimiter {
    tokens: AtomicU64,
    last_refill: AtomicI64,
    bytes_per_second: u64,
    tokens_per_interval: u64,
}

impl RateLimiter {
    pub fn new(speed_limit_kbps: u64) -> Self {
        let bytes_per_second = if speed_limit_kbps == 0 {
            u64::MAX
        } else {
            speed_limit_kbps * 1024
        };

        let tokens_per_interval = if bytes_per_second == u64::MAX {
            0
        } else {
            (bytes_per_second / (1000 / REFILL_INTERVAL_MS)).min(BUCKET_CAPACITY)
        };

        RateLimiter {
            tokens: AtomicU64::new(BUCKET_CAPACITY),
            last_refill: AtomicI64::new(0),
            bytes_per_second,
            tokens_per_interval,
        }
    }

    pub fn is_unlimited(&self) -> bool {
        self.bytes_per_second == u64::MAX
    }

    /// 尝试补充 tokens（基于时间间隔）
    fn try_refill(&self) {
        if self.is_unlimited() {
            return;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let last = self.last_refill.load(Ordering::Acquire);

        if now - last >= REFILL_INTERVAL_MS as i64
            && self
                .last_refill
                .compare_exchange(last, now, Ordering::SeqCst, Ordering::Acquire)
                .is_ok()
        {
            let current = self.tokens.load(Ordering::Acquire);
            let new_tokens = current
                .saturating_add(self.tokens_per_interval)
                .min(BUCKET_CAPACITY);
            self.tokens.store(new_tokens, Ordering::Release);
        }
    }

    /// 优化的 acquire：使用 fetch_sub 原子获取 + 后台补充
    pub async fn acquire(&self, bytes: usize) {
        if self.is_unlimited() {
            return;
        }

        let mut remaining = bytes as u64;

        while remaining > 0 {
            self.try_refill();

            let current = self.tokens.load(Ordering::Acquire);
            if current == 0 {
                let wait_ms =
                    ((remaining as f64 / self.bytes_per_second as f64) * 1000.0).ceil() as u64;
                tokio::time::sleep(Duration::from_millis(
                    wait_ms.clamp(REFILL_INTERVAL_MS, 100),
                ))
                .await;
                continue;
            }

            let to_consume = remaining.min(current);
            let actual = self.tokens.fetch_sub(to_consume, Ordering::SeqCst);
            let consumed = to_consume.min(actual);
            remaining -= consumed;

            if consumed < to_consume && actual > 0 {
                let refund = to_consume - consumed;
                self.tokens.fetch_add(refund, Ordering::SeqCst);
            }

            if remaining > 0 {
                tokio::time::sleep(Duration::from_millis(REFILL_INTERVAL_MS)).await;
            }
        }
    }

    pub fn get_available_tokens(&self) -> u64 {
        self.tokens.load(Ordering::Relaxed)
    }
}

pub struct TransferRateTracker {
    start_time: Instant,
    total_bytes: u64,
}

impl TransferRateTracker {
    pub fn new() -> Self {
        TransferRateTracker {
            start_time: Instant::now(),
            total_bytes: 0,
        }
    }

    pub fn add_bytes(&mut self, bytes: u64) {
        self.total_bytes += bytes;
    }

    pub fn get_rate_kbps(&self) -> u64 {
        let elapsed_secs = self.start_time.elapsed().as_secs_f64();
        if elapsed_secs > 0.0 {
            let bytes_per_sec = self.total_bytes as f64 / elapsed_secs;
            (bytes_per_sec / 1024.0) as u64
        } else {
            0
        }
    }

    pub fn get_total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn get_elapsed_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

impl Default for TransferRateTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct RateLimitConfig {
    pub speed_limit_kbps: u64,
}

impl RateLimitConfig {
    pub fn new(speed_limit_kbps: u64) -> Self {
        RateLimitConfig { speed_limit_kbps }
    }

    pub fn unlimited() -> Self {
        RateLimitConfig {
            speed_limit_kbps: 0,
        }
    }

    pub fn create_limiter(&self) -> RateLimiter {
        RateLimiter::new(self.speed_limit_kbps)
    }
}
