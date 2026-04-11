//! Property-based tests for the RateController.
//!
//! **Property 18: Rate Limiting Enforcement**
//! No session exceeds per-session limit; aggregate does not exceed global limit.
//! **Validates: Requirements 10.1, 10.2**
//!
//! **Property 19: Backpressure Hysteresis**
//! Backpressure engages above high-water mark, releases below low-water mark.
//! **Validates: Requirements 3.6, 10.3**

use proptest::prelude::*;
use transfer::rate_control::{BackpressureSignal, RateController, RateControllerConfig};

// ---------------------------------------------------------------------------
// Property 18: Rate Limiting Enforcement
// ---------------------------------------------------------------------------
//
// For any configured per-session rate limit R_s and global rate limit R_g,
// the rate controller SHALL not permit any single session to exceed R_s
// bytes/sec, and the aggregate of all sessions SHALL not exceed R_g bytes/sec.
//
// **Validates: Requirements 10.1, 10.2**
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn rate_limiting_enforcement(
        per_session_limit in 100u64..10_000,
        global_limit in 100u64..50_000,
        // Generate 1-4 sessions, each requesting 1-8 chunks
        session_count in 1usize..=4,
        chunk_requests in proptest::collection::vec(1usize..=500, 1..32),
    ) {
        // Use large queue thresholds so backpressure doesn't interfere
        let rc = RateController::new(RateControllerConfig {
            per_session_limit,
            global_limit,
            high_water_mark: usize::MAX / 2,
            low_water_mark: usize::MAX / 4,
            memory_threshold: usize::MAX,
            max_parallelism: 8,
        });

        // Track how many bytes each session successfully acquired
        let mut session_totals: Vec<usize> = vec![0; session_count];
        let mut global_total: usize = 0;

        for (i, &bytes) in chunk_requests.iter().enumerate() {
            let session_idx = i % session_count;
            let session_id = format!("session-{}", session_idx);

            match rc.acquire(&session_id, bytes) {
                Ok(()) => {
                    session_totals[session_idx] += bytes;
                    global_total += bytes;
                    // Immediately report transferred so queue doesn't fill
                    rc.report_transferred(&session_id, bytes);
                }
                Err(BackpressureSignal::SessionRateLimitExceeded { .. }) => {
                    // Expected: session would exceed its limit
                }
                Err(BackpressureSignal::GlobalRateLimitExceeded) => {
                    // Expected: global would exceed its limit
                }
                Err(BackpressureSignal::QueueBackpressure) => {
                    // Should not happen with huge thresholds, but not a violation
                }
            }
        }

        // Property: no session exceeded per-session limit (token bucket capacity)
        for (idx, &total) in session_totals.iter().enumerate() {
            prop_assert!(
                total <= per_session_limit as usize,
                "Session {} transferred {} bytes, exceeding per-session limit of {}",
                idx, total, per_session_limit
            );
        }

        // Property: aggregate did not exceed global limit (token bucket capacity)
        prop_assert!(
            global_total <= global_limit as usize,
            "Global total {} bytes exceeded global limit of {}",
            global_total, global_limit
        );
    }
}

// ---------------------------------------------------------------------------
// Property 19: Backpressure Hysteresis
// ---------------------------------------------------------------------------
//
// For any configured high-water mark H and low-water mark L (where H > L),
// the rate controller SHALL engage backpressure when the pending write queue
// size exceeds H, and SHALL not release backpressure until the queue size
// drops below L.
//
// **Validates: Requirements 3.6, 10.3**
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn backpressure_hysteresis(
        low_water in 10usize..1000,
        high_water_delta in 10usize..1000,
        // A sequence of operations: positive = acquire, negative = drain
        ops in proptest::collection::vec(-500i64..=500i64, 5..40),
    ) {
        let high_water = low_water + high_water_delta;

        let rc = RateController::new(RateControllerConfig {
            per_session_limit: 1_000_000, // large so rate limits don't interfere
            global_limit: 10_000_000,     // large so rate limits don't interfere
            high_water_mark: high_water,
            low_water_mark: low_water,
            memory_threshold: usize::MAX,
            max_parallelism: 8,
        });

        let mut was_engaged = false;

        for &op in &ops {
            if op > 0 {
                let bytes = op as usize;
                let result = rc.acquire("test-session", bytes);
                let queue_size = rc.pending_queue_size();
                let engaged = rc.is_backpressure_engaged();

                match result {
                    Ok(()) => {
                        // If we just transitioned to engaged, queue must have
                        // crossed high-water mark
                    }
                    Err(BackpressureSignal::QueueBackpressure) => {
                        // Backpressure is engaged
                        prop_assert!(
                            engaged,
                            "Got QueueBackpressure but is_backpressure_engaged is false"
                        );
                    }
                    Err(_) => {
                        // Rate limit errors are fine, not testing those here
                    }
                }

                // Key hysteresis property: if backpressure was engaged and
                // queue is still >= low_water, it must remain engaged
                if was_engaged && queue_size >= low_water {
                    prop_assert!(
                        engaged,
                        "Backpressure was engaged and queue_size={} >= low_water={}, \
                         but backpressure was released",
                        queue_size, low_water
                    );
                }

                was_engaged = engaged;
            } else if op < 0 {
                let bytes = (-op) as usize;
                rc.report_transferred("test-session", bytes);

                let queue_size = rc.pending_queue_size();

                // After draining, try a small acquire to trigger hysteresis check
                let _ = rc.acquire("test-session", 1);
                // Re-drain the 1 byte if it succeeded
                if rc.pending_queue_size() > queue_size {
                    rc.report_transferred("test-session", 1);
                }

                let engaged = rc.is_backpressure_engaged();

                // Key hysteresis property: if queue dropped below low_water,
                // backpressure must be released
                if rc.pending_queue_size() < low_water {
                    // After an acquire call that checks the state, backpressure
                    // should be released
                    prop_assert!(
                        !engaged,
                        "Queue size {} < low_water {} but backpressure still engaged",
                        rc.pending_queue_size(), low_water
                    );
                }

                was_engaged = engaged;
            }
            // op == 0: no-op
        }
    }
}
