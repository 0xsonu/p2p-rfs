//! Property tests for retry policy and error sanitization.

use proptest::prelude::*;
use std::time::Duration;

use file_sharing_server::retry::{ErrorResponse, InternalError, RetryPolicy};

proptest! {
    /// **Validates: Requirements 20.1**
    ///
    /// Property 25: Exponential Backoff Delay — Delay equals `min(B × 2^N, M)`
    /// for attempt N.
    #[test]
    fn exponential_backoff_delay(
        base_ms in 1u64..=1000,
        max_ms in 1u64..=120_000,
        attempt in 0u32..=15,
    ) {
        let base_delay = Duration::from_millis(base_ms);
        let max_delay = Duration::from_millis(max_ms);

        let policy = RetryPolicy {
            max_retries: 20,
            base_delay,
            max_delay,
        };

        let actual = policy.delay_for_attempt(attempt);

        // Expected: min(base × 2^min(attempt, 10), max)
        let capped_attempt = attempt.min(10);
        let expected_raw = base_delay * 2u32.pow(capped_attempt);
        let expected = expected_raw.min(max_delay);

        prop_assert_eq!(actual, expected,
            "attempt={}, base={}ms, max={}ms: expected {:?}, got {:?}",
            attempt, base_ms, max_ms, expected, actual
        );
    }

    /// **Validates: Requirements 20.4**
    ///
    /// Property 26: Error Sanitization — Client-facing error contains no
    /// internal details. For any internal error with arbitrary message and
    /// context strings, the resulting ErrorResponse must contain only a generic
    /// message and error code, with none of the internal details present.
    #[test]
    fn error_sanitization(
        message in "\\PC{1,200}",
        context in "\\PC{1,500}",
    ) {
        let internal = InternalError {
            message: message.clone(),
            context: context.clone(),
        };

        let response: ErrorResponse = internal.into();

        // Must return generic code and message
        prop_assert_eq!(response.code, 500);
        prop_assert_eq!(&response.message, "An internal error occurred");

        // The response message must NOT contain any of the internal details
        // (unless the internal detail happens to be a substring of the generic message)
        if !message.is_empty() && !"An internal error occurred".contains(&message) {
            prop_assert!(
                !response.message.contains(&message),
                "Response leaked internal message: '{}'", message
            );
        }
        if !context.is_empty() && !"An internal error occurred".contains(&context) {
            prop_assert!(
                !response.message.contains(&context),
                "Response leaked internal context: '{}'", context
            );
        }
    }
}
