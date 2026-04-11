//! Rate controller: per-session and global rate limiting with backpressure hysteresis.
//!
//! Uses a token-bucket algorithm for both per-session and global rate limits.
//! Implements backpressure using high-water/low-water marks on a pending write queue.
//! Adjusts recommended parallelism based on memory threshold.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use thiserror::Error;

/// Backpressure signal returned when rate limits or queue thresholds are exceeded.
#[derive(Debug, Error, PartialEq)]
pub enum BackpressureSignal {
    #[error("Per-session rate limit exceeded for session {session_id}")]
    SessionRateLimitExceeded { session_id: String },

    #[error("Global rate limit exceeded")]
    GlobalRateLimitExceeded,

    #[error("Backpressure engaged: pending queue above high-water mark")]
    QueueBackpressure,
}

/// A token bucket that refills at a given rate (bytes/sec).
#[derive(Debug)]
struct TokenBucket {
    /// Maximum tokens (capacity = rate limit in bytes/sec for a 1-second window)
    capacity: f64,
    /// Current available tokens
    tokens: f64,
    /// Last time tokens were refilled
    last_refill: Instant,
    /// Refill rate in bytes per second
    rate: f64,
}

impl TokenBucket {
    fn new(rate: u64) -> Self {
        let rate_f = rate as f64;
        Self {
            capacity: rate_f,
            tokens: rate_f,
            last_refill: Instant::now(),
            rate: rate_f,
        }
    }

    /// Refill tokens based on elapsed time since last refill.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.capacity);
        self.last_refill = now;
    }

    /// Try to consume `bytes` tokens. Returns true if successful.
    fn try_acquire(&mut self, bytes: usize) -> bool {
        self.refill();
        let needed = bytes as f64;
        if self.tokens >= needed {
            self.tokens -= needed;
            true
        } else {
            false
        }
    }
}

/// Configuration for the RateController.
#[derive(Debug, Clone)]
pub struct RateControllerConfig {
    /// Per-session rate limit in bytes/sec (Req 10.1)
    pub per_session_limit: u64,
    /// Global rate limit in bytes/sec (Req 10.2)
    pub global_limit: u64,
    /// High-water mark for pending write queue (Req 10.3)
    pub high_water_mark: usize,
    /// Low-water mark for pending write queue (Req 10.3)
    pub low_water_mark: usize,
    /// Memory threshold in bytes for reducing parallelism (Req 10.4)
    pub memory_threshold: usize,
    /// Maximum parallelism when memory is plentiful
    pub max_parallelism: usize,
}

/// Internal mutable state protected by a Mutex.
#[derive(Debug)]
struct RateControllerState {
    /// Per-session token buckets
    session_buckets: HashMap<String, TokenBucket>,
    /// Global token bucket
    global_bucket: TokenBucket,
    /// Current pending write queue size in bytes
    pending_queue_size: usize,
    /// Whether backpressure is currently engaged
    backpressure_engaged: bool,
    /// Current memory usage estimate for transfer buffers
    current_memory_usage: usize,
}

/// Rate controller with per-session and global rate limiting, backpressure
/// hysteresis, and adaptive parallelism.
pub struct RateController {
    config: RateControllerConfig,
    state: Mutex<RateControllerState>,
}

impl RateController {
    /// Create a new RateController with the given configuration.
    pub fn new(config: RateControllerConfig) -> Self {
        let state = RateControllerState {
            session_buckets: HashMap::new(),
            global_bucket: TokenBucket::new(config.global_limit),
            pending_queue_size: 0,
            backpressure_engaged: false,
            current_memory_usage: 0,
        };
        Self {
            config,
            state: Mutex::new(state),
        }
    }

    /// Check if a transfer of `bytes` for `session_id` can proceed.
    ///
    /// Returns `Ok(())` if the transfer is allowed, or a `BackpressureSignal`
    /// if any limit is exceeded.
    ///
    /// Token-bucket algorithm:
    /// - Each session has its own bucket refilling at `per_session_limit` bytes/sec
    /// - A global bucket refills at `global_limit` bytes/sec
    /// - Both must have sufficient tokens for the request to proceed
    ///
    /// Backpressure hysteresis (Req 10.3):
    /// - Engages when pending_queue_size > high_water_mark
    /// - Releases when pending_queue_size < low_water_mark
    pub fn acquire(
        &self,
        session_id: &str,
        bytes: usize,
    ) -> Result<(), BackpressureSignal> {
        let mut state = self.state.lock().unwrap();

        // Check backpressure hysteresis first
        if state.backpressure_engaged {
            if state.pending_queue_size < self.config.low_water_mark {
                state.backpressure_engaged = false;
            } else {
                return Err(BackpressureSignal::QueueBackpressure);
            }
        } else if state.pending_queue_size > self.config.high_water_mark {
            state.backpressure_engaged = true;
            return Err(BackpressureSignal::QueueBackpressure);
        }

        // Check per-session rate limit
        let session_bucket = state
            .session_buckets
            .entry(session_id.to_string())
            .or_insert_with(|| TokenBucket::new(self.config.per_session_limit));

        if !session_bucket.try_acquire(bytes) {
            return Err(BackpressureSignal::SessionRateLimitExceeded {
                session_id: session_id.to_string(),
            });
        }

        // Check global rate limit
        if !state.global_bucket.try_acquire(bytes) {
            // Refund the session tokens since global limit blocked us
            let bucket = state.session_buckets.get_mut(session_id).unwrap();
            bucket.tokens = (bucket.tokens + bytes as f64).min(bucket.capacity);
            return Err(BackpressureSignal::GlobalRateLimitExceeded);
        }

        // Track pending queue size
        state.pending_queue_size += bytes;

        Ok(())
    }

    /// Report that `bytes` have been transferred (written to disk) for a session.
    /// This decreases the pending queue size.
    pub fn report_transferred(&self, _session_id: &str, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.pending_queue_size = state.pending_queue_size.saturating_sub(bytes);
    }

    /// Set the current memory usage estimate for transfer buffers.
    /// Used by `recommended_parallelism()` to adjust concurrency.
    pub fn set_memory_usage(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.current_memory_usage = bytes;
    }

    /// Get the recommended number of parallel streams based on memory usage (Req 10.4).
    ///
    /// When memory usage is below the threshold, returns max_parallelism.
    /// When at or above the threshold, reduces parallelism proportionally,
    /// with a minimum of 1.
    pub fn recommended_parallelism(&self) -> usize {
        let state = self.state.lock().unwrap();
        if self.config.memory_threshold == 0 {
            return 1;
        }
        if state.current_memory_usage >= self.config.memory_threshold {
            return 1;
        }
        // Scale linearly: full parallelism at 0 usage, 1 at threshold
        let ratio =
            1.0 - (state.current_memory_usage as f64 / self.config.memory_threshold as f64);
        let parallelism = (ratio * self.config.max_parallelism as f64).ceil() as usize;
        parallelism.max(1).min(self.config.max_parallelism)
    }

    /// Get the current pending queue size (for testing/observability).
    pub fn pending_queue_size(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.pending_queue_size
    }

    /// Check if backpressure is currently engaged (for testing/observability).
    pub fn is_backpressure_engaged(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.backpressure_engaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_acquire_succeeds() {
        let rc = RateController::new(RateControllerConfig {
            per_session_limit: 1000,
            global_limit: 5000,
            high_water_mark: 10000,
            low_water_mark: 5000,
            memory_threshold: 1_000_000,
            max_parallelism: 8,
        });
        assert!(rc.acquire("s1", 500).is_ok());
    }

    #[test]
    fn session_rate_limit_exceeded() {
        let rc = RateController::new(RateControllerConfig {
            per_session_limit: 100,
            global_limit: 10000,
            high_water_mark: 100000,
            low_water_mark: 50000,
            memory_threshold: 1_000_000,
            max_parallelism: 8,
        });
        // First acquire uses up most tokens
        assert!(rc.acquire("s1", 100).is_ok());
        // Second should fail - no tokens left
        let result = rc.acquire("s1", 50);
        assert!(matches!(
            result,
            Err(BackpressureSignal::SessionRateLimitExceeded { .. })
        ));
    }

    #[test]
    fn global_rate_limit_exceeded() {
        let rc = RateController::new(RateControllerConfig {
            per_session_limit: 10000,
            global_limit: 100,
            high_water_mark: 100000,
            low_water_mark: 50000,
            memory_threshold: 1_000_000,
            max_parallelism: 8,
        });
        assert!(rc.acquire("s1", 100).is_ok());
        // Global bucket exhausted
        let result = rc.acquire("s2", 50);
        assert!(matches!(
            result,
            Err(BackpressureSignal::GlobalRateLimitExceeded)
        ));
    }

    #[test]
    fn backpressure_hysteresis() {
        let rc = RateController::new(RateControllerConfig {
            per_session_limit: 100000,
            global_limit: 100000,
            high_water_mark: 100,
            low_water_mark: 50,
            memory_threshold: 1_000_000,
            max_parallelism: 8,
        });

        // Fill queue above high-water mark
        assert!(rc.acquire("s1", 101).is_ok());
        // Now backpressure should be engaged
        let result = rc.acquire("s1", 10);
        assert!(matches!(result, Err(BackpressureSignal::QueueBackpressure)));

        // Drain below low-water mark
        rc.report_transferred("s1", 70); // 101 - 70 = 31 < 50
        // Should release backpressure
        assert!(rc.acquire("s1", 10).is_ok());
    }

    #[test]
    fn recommended_parallelism_scales() {
        let rc = RateController::new(RateControllerConfig {
            per_session_limit: 10000,
            global_limit: 100000,
            high_water_mark: 100000,
            low_water_mark: 50000,
            memory_threshold: 1000,
            max_parallelism: 8,
        });

        // No memory usage -> max parallelism
        assert_eq!(rc.recommended_parallelism(), 8);

        // At threshold -> minimum parallelism
        rc.set_memory_usage(1000);
        assert_eq!(rc.recommended_parallelism(), 1);

        // Above threshold -> minimum parallelism
        rc.set_memory_usage(2000);
        assert_eq!(rc.recommended_parallelism(), 1);
    }

    #[test]
    fn report_transferred_decreases_queue() {
        let rc = RateController::new(RateControllerConfig {
            per_session_limit: 10000,
            global_limit: 100000,
            high_water_mark: 100000,
            low_water_mark: 50000,
            memory_threshold: 1_000_000,
            max_parallelism: 8,
        });
        assert!(rc.acquire("s1", 500).is_ok());
        assert_eq!(rc.pending_queue_size(), 500);
        rc.report_transferred("s1", 300);
        assert_eq!(rc.pending_queue_size(), 200);
    }
}
