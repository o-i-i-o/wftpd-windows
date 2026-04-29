//! Transfer rate limiter
//!
//! Implements user upload/download speed limiting using token bucket algorithm
//! Dynamically adjusts bucket capacity based on rate limit

use std::pin::Pin;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, ReadBuf};

const MIN_BUCKET_CAPACITY: u64 = 64 * 1024;
const MAX_BUCKET_CAPACITY: u64 = 1024 * 1024;
const REFILL_INTERVAL_MS: u64 = 10;

/// High-performance rate limiter: using token bucket algorithm + background refill
pub struct RateLimiter {
    tokens: AtomicU64,
    last_refill: AtomicI64,
    bytes_per_second: u64,
    tokens_per_interval: u64,
    bucket_capacity: u64,
}

impl RateLimiter {
    pub fn new(speed_limit_kbps: u64) -> Self {
        let bytes_per_second = if speed_limit_kbps == 0 {
            u64::MAX
        } else {
            speed_limit_kbps * 1024
        };

        let bucket_capacity = if bytes_per_second == u64::MAX {
            MIN_BUCKET_CAPACITY
        } else {
            let ideal_capacity = bytes_per_second;
            ideal_capacity.clamp(MIN_BUCKET_CAPACITY, MAX_BUCKET_CAPACITY)
        };

        let tokens_per_interval = if bytes_per_second == u64::MAX {
            0
        } else {
            (bytes_per_second / (1000 / REFILL_INTERVAL_MS)).min(bucket_capacity)
        };

        RateLimiter {
            tokens: AtomicU64::new(bucket_capacity),
            last_refill: AtomicI64::new(0),
            bytes_per_second,
            tokens_per_interval,
            bucket_capacity,
        }
    }

    pub fn is_unlimited(&self) -> bool {
        self.bytes_per_second == u64::MAX
    }

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
                .min(self.bucket_capacity);
            self.tokens.store(new_tokens, Ordering::Release);
        }
    }

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

    pub fn consume_tokens(&self, count: u64) {
        if self.is_unlimited() || count == 0 {
            return;
        }
        let actual = self.tokens.fetch_sub(count, Ordering::SeqCst);
        if actual < count {
            self.tokens.fetch_add(count - actual, Ordering::SeqCst);
        }
    }

    pub fn get_bucket_capacity(&self) -> u64 {
        self.bucket_capacity
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

pub struct RateLimitedReader<R> {
    inner: R,
    limiter: RateLimiter,
}

impl<R> RateLimitedReader<R> {
    pub fn new(inner: R, speed_limit_kbps: u64) -> Self {
        RateLimitedReader {
            inner,
            limiter: RateLimiter::new(speed_limit_kbps),
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for RateLimitedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let remaining = buf.remaining();
        if remaining == 0 {
            return Poll::Ready(Ok(()));
        }

        if self.limiter.is_unlimited() {
            return Pin::new(&mut self.inner).poll_read(cx, buf);
        }

        self.limiter.try_refill();

        let available = self.limiter.get_available_tokens() as usize;
        if available == 0 {
            let bytes_per_second = self.limiter.bytes_per_second;
            let wait_ms = ((remaining as f64 / bytes_per_second as f64) * 1000.0).ceil() as u64;
            let wait_ms = wait_ms.clamp(REFILL_INTERVAL_MS, 100);

            let waker = cx.waker().clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                waker.wake();
            });
            return Poll::Pending;
        }

        let to_read = remaining.min(available);
        let mut limited_buf = ReadBuf::new(&mut buf.initialize_unfilled()[..to_read]);

        match Pin::new(&mut self.inner).poll_read(cx, &mut limited_buf) {
            Poll::Ready(Ok(())) => {
                let n = limited_buf.filled().len();
                if n > 0 {
                    self.limiter.consume_tokens(n as u64);
                    unsafe {
                        buf.assume_init(n);
                    }
                    buf.advance(n);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_unlimited() {
        let limiter = RateLimiter::new(0);
        assert!(limiter.is_unlimited());
        assert_eq!(limiter.bytes_per_second, u64::MAX);
    }

    #[test]
    fn test_rate_limiter_limited() {
        let limiter = RateLimiter::new(1024);
        assert!(!limiter.is_unlimited());
        assert_eq!(limiter.bytes_per_second, 1024 * 1024);
    }

    #[test]
    fn test_rate_limiter_initial_tokens() {
        let limiter = RateLimiter::new(1024);
        let tokens = limiter.get_available_tokens();
        assert!(tokens > 0);
        assert!(tokens <= limiter.get_bucket_capacity());
    }

    #[test]
    fn test_rate_limiter_bucket_capacity_low_rate() {
        let limiter = RateLimiter::new(10);
        assert_eq!(limiter.get_bucket_capacity(), MIN_BUCKET_CAPACITY);
    }

    #[test]
    fn test_rate_limiter_bucket_capacity_medium_rate() {
        let limiter = RateLimiter::new(512);
        let expected = 512 * 1024;
        assert_eq!(limiter.get_bucket_capacity(), expected);
    }

    #[test]
    fn test_rate_limiter_bucket_capacity_high_rate() {
        let limiter = RateLimiter::new(10240);
        assert_eq!(limiter.get_bucket_capacity(), MAX_BUCKET_CAPACITY);
    }

    #[test]
    fn test_rate_limit_config_new() {
        let config = RateLimitConfig::new(1024);
        assert_eq!(config.speed_limit_kbps, 1024);
    }

    #[test]
    fn test_rate_limit_config_unlimited() {
        let config = RateLimitConfig::unlimited();
        assert_eq!(config.speed_limit_kbps, 0);
    }

    #[test]
    fn test_rate_limit_config_create_limiter() {
        let config = RateLimitConfig::new(512);
        let limiter = config.create_limiter();
        assert!(!limiter.is_unlimited());
    }

    #[test]
    fn test_transfer_rate_tracker_new() {
        let tracker = TransferRateTracker::new();
        assert_eq!(tracker.get_total_bytes(), 0);
        assert_eq!(tracker.get_rate_kbps(), 0);
    }

    #[test]
    fn test_transfer_rate_tracker_add_bytes() {
        let mut tracker = TransferRateTracker::new();
        tracker.add_bytes(1024);
        assert_eq!(tracker.get_total_bytes(), 1024);
    }

    #[test]
    fn test_transfer_rate_tracker_elapsed() {
        let tracker = TransferRateTracker::new();
        std::thread::sleep(Duration::from_millis(1100));
        assert!(tracker.get_elapsed_secs() >= 1);
    }

    #[test]
    fn test_transfer_rate_tracker_default() {
        let tracker: TransferRateTracker = Default::default();
        assert_eq!(tracker.get_total_bytes(), 0);
    }

    #[tokio::test]
    async fn test_rate_limiter_acquire_unlimited() {
        let limiter = RateLimiter::new(0);
        limiter.acquire(1024 * 1024).await;
    }

    #[tokio::test]
    async fn test_rate_limiter_acquire_small_amount() {
        let limiter = RateLimiter::new(10240);
        limiter.acquire(100).await;
    }

    #[tokio::test]
    async fn test_rate_limiter_refill() {
        let limiter = RateLimiter::new(1024);
        let initial = limiter.get_available_tokens();

        limiter.try_refill();

        let after = limiter.get_available_tokens();
        assert!(after >= initial || after == limiter.get_bucket_capacity());
    }
}
