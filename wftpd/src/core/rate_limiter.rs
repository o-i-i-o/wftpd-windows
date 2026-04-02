use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicU64, Ordering};

const BUCKET_CAPACITY: u64 = 64 * 1024;

/// 高性能限流器：使用原子操作减少锁竞争
pub struct RateLimiter {
    // 使用原子操作存储 tokens，减少锁竞争
    tokens: AtomicU64,
    bytes_per_second: u64,
}

impl RateLimiter {
    pub fn new(speed_limit_kbps: u64) -> Self {
        let bytes_per_second = if speed_limit_kbps == 0 {
            u64::MAX
        } else {
            speed_limit_kbps * 1024
        };
        
        RateLimiter {
            tokens: AtomicU64::new(BUCKET_CAPACITY),
            bytes_per_second,
        }
    }

    pub fn is_unlimited(&self) -> bool {
        self.bytes_per_second == u64::MAX
    }

    /// 优化的 acquire：使用自适应等待减少 CPU 忙循环
    pub async fn acquire(&self, bytes: usize) {
        if self.is_unlimited() {
            return;
        }

        let mut remaining = bytes as u64;
        let mut backoff_ms: u64 = 1;

        while remaining > 0 {
            let current_tokens = self.tokens.load(Ordering::Acquire);

            if current_tokens > 0 {
                let to_consume = remaining.min(current_tokens);
                match self.tokens.compare_exchange_weak(
                    current_tokens,
                    current_tokens - to_consume,
                    Ordering::SeqCst,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        remaining -= to_consume;
                        backoff_ms = 1; // 成功后重置退避
                        continue;
                    }
                    Err(_) => {
                        // CAS 失败，短暂退避后重试
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        backoff_ms = (backoff_ms * 2).min(8); // 指数退避，最大 8ms
                    }
                }
            } else {
                // 无可用 token，计算精确等待时间
                let wait_ms = ((remaining as f64 / self.bytes_per_second as f64) * 1000.0).ceil() as u64;
                let actual_wait = wait_ms.max(backoff_ms).min(100); // 限制最大等待时间

                tokio::time::sleep(Duration::from_millis(actual_wait)).await;

                // 补充 tokens（基于等待时间）
                let tokens_per_ms = self.bytes_per_second / 1000;
                let refill_amount = (tokens_per_ms * actual_wait).min(BUCKET_CAPACITY);
                let _ = self.tokens.fetch_add(refill_amount, Ordering::Relaxed);

                backoff_ms = (backoff_ms * 2).min(16); // 增加退避时间
            }
        }
    }

    pub async fn get_available_tokens(&self) -> u64 {
        // 优先使用原子读取
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
        RateLimitConfig { speed_limit_kbps: 0 }
    }

    pub fn create_limiter(&self) -> RateLimiter {
        RateLimiter::new(self.speed_limit_kbps)
    }
}
