use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

const BUCKET_CAPACITY: u64 = 64 * 1024;
const REFILL_INTERVAL_MS: u64 = 100;

/// 高性能限流器：使用原子操作减少锁竞争
pub struct RateLimiter {
    // 使用原子操作存储 tokens，减少锁竞争
    tokens: AtomicU64,
    last_refill: AtomicU64, // 毫秒时间戳
    bytes_per_second: u64,
}

struct RateLimiterState {
    tokens: u64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(speed_limit_kbps: u64) -> Self {
        let bytes_per_second = if speed_limit_kbps == 0 {
            u64::MAX
        } else {
            speed_limit_kbps * 1024
        };
        
        let now_ms = Instant::now().duration_since(Instant::now()).as_millis() as u64;
        
        RateLimiter {
            tokens: AtomicU64::new(BUCKET_CAPACITY),
            last_refill: AtomicU64::new(now_ms),
            bytes_per_second,
        }
    }

    pub fn is_unlimited(&self) -> bool {
        self.bytes_per_second == u64::MAX
    }

    /// 优化的 acquire：优先使用原子操作，减少锁竞争
    pub async fn acquire(&self, bytes: usize) {
        if self.is_unlimited() {
            return;
        }
        
        let mut remaining = bytes as u64;
        
        while remaining > 0 {
            // 快速路径：尝试使用原子操作获取 tokens
            let current_tokens = self.tokens.load(Ordering::Relaxed);
            
            if current_tokens > 0 {
                let to_consume = remaining.min(current_tokens);
                // 尝试原子扣减
                if self.tokens.compare_exchange(
                    current_tokens,
                    current_tokens - to_consume,
                    Ordering::SeqCst,
                    Ordering::Relaxed
                ).is_ok() {
                    remaining -= to_consume;
                    continue;
                }
                // CAS 失败，重试
            }
            
            // 慢速路径：需要补充 tokens
            // 计算需要等待的时间
            let tokens_per_interval = self.bytes_per_second / (1000 / REFILL_INTERVAL_MS);
            let refill_amount = tokens_per_interval.min(BUCKET_CAPACITY);
            let wait_ms = if self.bytes_per_second > 0 {
                (refill_amount as f64 / self.bytes_per_second as f64 * 1000.0) as u64
            } else {
                REFILL_INTERVAL_MS
            };
            
            tokio::time::sleep(Duration::from_millis(wait_ms.max(1))).await;
            
            // 补充 tokens
            let current = self.tokens.load(Ordering::Relaxed);
            let new_tokens = current.saturating_add(refill_amount).min(BUCKET_CAPACITY);
            self.tokens.store(new_tokens, Ordering::Relaxed);
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
