use observability::{EventType, ObservabilityModule, TransferEvent};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn arb_event_type() -> impl Strategy<Value = EventType> {
    prop_oneof![
        Just(EventType::Start),
        Just(EventType::ChunkComplete),
        Just(EventType::Complete),
        Just(EventType::Failed),
        Just(EventType::Retry),
    ]
}

fn arb_transfer_event() -> impl Strategy<Value = TransferEvent> {
    (
        "[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}", // correlation_id
        "[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}", // session_id
        arb_event_type(),
    )
        .prop_map(|(correlation_id, session_id, event_type)| TransferEvent {
            correlation_id,
            session_id,
            event_type,
            timestamp: chrono::Utc::now(),
            details: serde_json::json!({}),
            file_id: None,
            failed_chunk_indices: None,
            failure_reason: None,
        })
}

fn arb_session_id() -> impl Strategy<Value = String> {
    "[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}"
}

fn arb_file_id() -> impl Strategy<Value = String> {
    "[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}"
}

// ---------------------------------------------------------------------------
// Property 20: Structured Log Entry Format
// **Validates: Requirements 11.1, 11.4**
//
// For any transfer event (start, completion, failure, retry), the emitted log
// entry SHALL be valid JSON containing at minimum: event_type, session_id,
// and timestamp. For failure events after retry exhaustion, the log SHALL
// additionally contain file_id, failed_chunk_indices, and failure_reason.
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 20 – part A: every `log_transfer_event` entry is valid JSON
    /// with the required fields (event_type, session_id, timestamp,
    /// correlation_id).
    #[test]
    fn prop20_log_entry_is_valid_json_with_required_fields(event in arb_transfer_event()) {
        let module = ObservabilityModule::new();
        module.log_transfer_event(event.clone());

        let entries = module.log_entries();
        prop_assert_eq!(entries.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(&entries[0])
            .expect("log entry must be valid JSON");

        // Required fields
        prop_assert!(parsed.get("event_type").is_some(), "missing event_type");
        prop_assert!(parsed.get("session_id").is_some(), "missing session_id");
        prop_assert!(parsed.get("timestamp").is_some(), "missing timestamp");
        prop_assert!(parsed.get("correlation_id").is_some(), "missing correlation_id");
    }

    /// Property 20 – part B: failure logs from `log_transfer_failure` contain
    /// the additional context fields (file_id, failed_chunk_indices,
    /// failure_reason).
    #[test]
    fn prop20_failure_log_contains_additional_context(
        session_id in arb_session_id(),
        file_id in arb_file_id(),
        failed_chunks in prop::collection::vec(0u64..1000, 1..10),
        reason in "[a-zA-Z0-9 _-]{1,64}",
    ) {
        let module = ObservabilityModule::new();
        module.log_transfer_failure(session_id.clone(), file_id.clone(), &failed_chunks, &reason);

        let entries = module.log_entries();
        prop_assert_eq!(entries.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(&entries[0])
            .expect("failure log entry must be valid JSON");

        // Standard fields
        prop_assert!(parsed.get("event_type").is_some(), "missing event_type");
        prop_assert!(parsed.get("session_id").is_some(), "missing session_id");
        prop_assert!(parsed.get("timestamp").is_some(), "missing timestamp");
        prop_assert!(parsed.get("correlation_id").is_some(), "missing correlation_id");

        // Additional failure context
        prop_assert!(parsed.get("file_id").is_some(), "missing file_id");
        prop_assert!(parsed.get("failed_chunk_indices").is_some(), "missing failed_chunk_indices");
        prop_assert!(parsed.get("failure_reason").is_some(), "missing failure_reason");

        // Values match inputs
        prop_assert_eq!(parsed["session_id"].as_str().unwrap(), session_id.as_str());
        prop_assert_eq!(parsed["file_id"].as_str().unwrap(), file_id.as_str());
        prop_assert_eq!(parsed["failure_reason"].as_str().unwrap(), reason.as_str());

        let logged_chunks: Vec<u64> = serde_json::from_value(parsed["failed_chunk_indices"].clone()).unwrap();
        prop_assert_eq!(logged_chunks, failed_chunks);
    }
}

// ---------------------------------------------------------------------------
// Property 21: Correlation ID Consistency
// **Validates: Requirements 11.5**
//
// For any TransferSession, all log entries emitted during that session's
// lifecycle SHALL contain the same correlation_id value.
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 21: multiple events logged for the same session all share the
    /// same correlation_id.
    #[test]
    fn prop21_correlation_id_consistent_across_session(
        correlation_id in arb_session_id(),
        session_id in arb_session_id(),
        event_types in prop::collection::vec(arb_event_type(), 2..10),
    ) {
        let module = ObservabilityModule::new();

        for et in &event_types {
            let event = TransferEvent {
                correlation_id: correlation_id.clone(),
                session_id: session_id.clone(),
                event_type: et.clone(),
                timestamp: chrono::Utc::now(),
                details: serde_json::json!({}),
                file_id: None,
                failed_chunk_indices: None,
                failure_reason: None,
            };
            module.log_transfer_event(event);
        }

        let entries = module.log_entries();
        prop_assert_eq!(entries.len(), event_types.len());

        for entry_str in &entries {
            let parsed: serde_json::Value = serde_json::from_str(entry_str)
                .expect("log entry must be valid JSON");
            prop_assert_eq!(
                parsed["correlation_id"].as_str().unwrap(),
                correlation_id.as_str(),
                "correlation_id mismatch in entry"
            );
        }
    }
}
