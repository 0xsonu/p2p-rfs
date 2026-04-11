//! Property-based tests for chunk layout computation and TransferSession persistence.
//!
//! **Property 5: Chunk Layout Covers Entire File**
//! Chunks cover file size exactly with sequential indices from 0 to total_chunks - 1.
//! **Validates: Requirements 3.1**
//!
//! **Property 8: Transfer Session State Round-Trip**
//! Serialize then deserialize produces equivalent session with identical
//! completed chunk indices, hashes, and metadata.
//! **Validates: Requirements 5.1**

use chrono::{TimeZone, Utc};
use proptest::prelude::*;
use std::collections::{BTreeSet, HashMap};
use transfer::session::{
    chunk_offset, compute_chunk_layout, FileMeta, TransferDirection, TransferSession,
    TransferStatus,
};

// ---------------------------------------------------------------------------
// Property 5: Chunk Layout Covers Entire File
// ---------------------------------------------------------------------------
//
// For any file size > 0 and chunk size > 0, the computed chunk layout SHALL
// produce chunks whose total byte coverage equals the file size exactly,
// with sequential indices from 0 to total_chunks - 1.
//
// **Validates: Requirements 3.1**
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn chunk_layout_covers_entire_file(
        file_size in 1u64..10_000_000,
        chunk_size in 1usize..1_000_000,
    ) {
        let (total_chunks, last_chunk_size) = compute_chunk_layout(file_size, chunk_size);

        // Must have at least one chunk for non-zero file
        prop_assert!(total_chunks > 0, "file_size={} should produce at least 1 chunk", file_size);

        // Last chunk size must be in [1, chunk_size]
        prop_assert!(
            last_chunk_size >= 1 && last_chunk_size <= chunk_size,
            "last_chunk_size={} out of range [1, {}]",
            last_chunk_size, chunk_size
        );

        // Total byte coverage: (total_chunks - 1) * chunk_size + last_chunk_size == file_size
        let coverage = (total_chunks - 1) * chunk_size as u64 + last_chunk_size as u64;
        prop_assert_eq!(
            coverage, file_size,
            "Chunk coverage {} != file_size {}",
            coverage, file_size
        );

        // Sequential offsets: chunk_offset(i) for i in 0..total_chunks
        for i in 0..total_chunks {
            let expected_offset = i * chunk_size as u64;
            let actual_offset = chunk_offset(i, chunk_size);
            prop_assert_eq!(
                actual_offset, expected_offset,
                "chunk_offset({}, {}) = {} != expected {}",
                i, chunk_size, actual_offset, expected_offset
            );
        }

        // Last chunk ends exactly at file_size
        let last_offset = chunk_offset(total_chunks - 1, chunk_size);
        prop_assert_eq!(
            last_offset + last_chunk_size as u64, file_size,
            "Last chunk end {} != file_size {}",
            last_offset + last_chunk_size as u64, file_size
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers to build arbitrary TransferSession values
// ---------------------------------------------------------------------------

/// Build a TransferSession from a small set of seeds to stay within proptest
/// tuple-size limits.
fn build_session(
    seed_id: u64,
    file_size: u64,
    chunk_size: usize,
    direction_flag: bool,
    status_flag: u8,
    completed_ratio: f64,
    ts_secs: i64,
) -> TransferSession {
    let (total_chunks, _) = compute_chunk_layout(file_size.max(1), chunk_size.max(1));
    let chunk_size = chunk_size.max(1);
    let file_size = file_size.max(1);

    let id = format!("sess-{:x}", seed_id);
    let file_id = format!("file-{:x}", seed_id.wrapping_add(1));
    let user_id = format!("user-{:x}", seed_id.wrapping_add(2));

    let direction = if direction_flag {
        TransferDirection::Upload
    } else {
        TransferDirection::Download
    };

    let status = match status_flag % 4 {
        0 => TransferStatus::InProgress,
        1 => TransferStatus::Paused,
        2 => TransferStatus::Completed,
        _ => TransferStatus::Failed {
            reason: format!("error-{}", seed_id),
        },
    };

    // Build completed chunks as a fraction of total
    let num_completed =
        ((total_chunks as f64 * completed_ratio.clamp(0.0, 1.0)) as u64).min(total_chunks);
    let completed_chunks: BTreeSet<u64> = (0..num_completed).collect();

    // Build chunk hashes for completed chunks
    let chunk_hashes: HashMap<u64, String> = completed_chunks
        .iter()
        .map(|&idx| (idx, format!("{:064x}", idx)))
        .collect();

    // Build some retry counts
    let retry_counts: HashMap<u64, u32> = if total_chunks > 0 {
        let retry_idx = num_completed.min(total_chunks - 1);
        let mut m = HashMap::new();
        m.insert(retry_idx, (seed_id % 5) as u32);
        m
    } else {
        HashMap::new()
    };

    let dt = Utc.timestamp_opt(ts_secs.clamp(1_000_000_000, 1_999_999_999), 0)
        .unwrap();

    let file_meta = FileMeta {
        file_id: file_id.clone(),
        filename: format!("file_{}.dat", seed_id),
        size: file_size,
        mime_type: Some("application/octet-stream".to_string()),
        chunk_size,
        total_chunks,
        whole_file_hash: format!("{:064x}", seed_id),
        hash_algorithm: "sha256".to_string(),
        uploaded_by: user_id.clone(),
        uploaded_at: dt,
    };

    TransferSession {
        id,
        file_id,
        user_id,
        direction,
        file_meta,
        chunk_size,
        total_chunks,
        completed_chunks,
        chunk_hashes,
        status,
        created_at: dt,
        updated_at: dt,
        retry_counts,
    }
}

// ---------------------------------------------------------------------------
// Property 8: Transfer Session State Round-Trip
// ---------------------------------------------------------------------------
//
// For any valid TransferSession object, serializing it to the persistence
// format and then deserializing SHALL produce an equivalent TransferSession
// with identical completed chunk indices, hashes, and metadata.
//
// **Validates: Requirements 5.1**
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn transfer_session_state_round_trip(
        seed_id in 0u64..1_000_000,
        file_size in 1u64..10_000_000,
        chunk_size in 1usize..1_000_000,
        direction_flag in proptest::bool::ANY,
        status_flag in 0u8..4,
        completed_ratio in 0.0f64..1.0,
        ts_secs in 1_000_000_000i64..2_000_000_000,
    ) {
        let session = build_session(
            seed_id, file_size, chunk_size, direction_flag,
            status_flag, completed_ratio, ts_secs,
        );

        // Serialize to JSON
        let json = serde_json::to_string(&session)
            .expect("TransferSession should serialize to JSON");

        // Deserialize back
        let deserialized: TransferSession = serde_json::from_str(&json)
            .expect("TransferSession should deserialize from JSON");

        // Full equality
        prop_assert_eq!(&session, &deserialized, "Round-trip produced different session");

        // Explicitly verify critical fields per the property statement
        prop_assert_eq!(
            &session.completed_chunks, &deserialized.completed_chunks,
            "completed_chunks mismatch"
        );
        prop_assert_eq!(
            &session.chunk_hashes, &deserialized.chunk_hashes,
            "chunk_hashes mismatch"
        );
        prop_assert_eq!(
            &session.file_meta, &deserialized.file_meta,
            "file_meta mismatch"
        );
        prop_assert_eq!(
            session.total_chunks, deserialized.total_chunks,
            "total_chunks mismatch"
        );
        prop_assert_eq!(
            session.chunk_size, deserialized.chunk_size,
            "chunk_size mismatch"
        );
        prop_assert_eq!(
            &session.status, &deserialized.status,
            "status mismatch"
        );
        prop_assert_eq!(
            &session.retry_counts, &deserialized.retry_counts,
            "retry_counts mismatch"
        );
    }
}
