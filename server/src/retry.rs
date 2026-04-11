//! Retry policy with exponential backoff and error sanitization.
//!
//! Requirements: 20.1, 20.2, 20.3, 20.4

use std::time::Duration;

/// Configurable retry policy with exponential backoff.
///
/// Delay for attempt N = min(base_delay × 2^N, max_delay).
/// The exponent is capped at 10 to prevent overflow.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl RetryPolicy {
    /// Compute the delay before the Nth retry attempt.
    ///
    /// Returns `min(base_delay × 2^attempt, max_delay)`.
    /// The exponent is capped at 10 to avoid overflow in `Duration` arithmetic.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay = self.base_delay * 2u32.pow(attempt.min(10));
        delay.min(self.max_delay)
    }
}

/// Internal error carrying full diagnostic context.
/// This MUST NOT be sent to clients (Req 20.4).
#[derive(Debug, thiserror::Error)]
#[error("Internal error: {message}")]
pub struct InternalError {
    pub message: String,
    pub context: String,
}

/// Client-facing error response with no internal details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorResponse {
    pub code: u32,
    pub message: String,
}

impl From<InternalError> for ErrorResponse {
    fn from(err: InternalError) -> Self {
        // Log full error internally — never expose to client
        tracing::error!(
            error.message = %err.message,
            error.context = %err.context,
            "Internal error occurred"
        );
        ErrorResponse {
            code: 500,
            message: "An internal error occurred".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_exponential_backoff() {
        let policy = RetryPolicy {
            max_retries: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
        };
        assert_eq!(policy.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(800));
    }

    #[test]
    fn backoff_capped_at_max_delay() {
        let policy = RetryPolicy {
            max_retries: 20,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
        };
        // 2^10 = 1024 seconds > 60 seconds max
        assert_eq!(policy.delay_for_attempt(10), Duration::from_secs(60));
        assert_eq!(policy.delay_for_attempt(15), Duration::from_secs(60));
    }

    #[test]
    fn error_sanitization_strips_internal_details() {
        let internal = InternalError {
            message: "database connection pool exhausted".to_string(),
            context: "stack trace at db.rs:42, connection string: postgres://secret@host/db".to_string(),
        };
        let response: ErrorResponse = internal.into();
        assert_eq!(response.code, 500);
        assert_eq!(response.message, "An internal error occurred");
        assert!(!response.message.contains("database"));
        assert!(!response.message.contains("stack trace"));
        assert!(!response.message.contains("postgres"));
    }
}
