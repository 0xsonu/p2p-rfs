//! Transfer session management: chunk layout computation, session state, and persistence.
//!
//! Provides the core `TransferSession` struct that tracks the state of an in-progress
//! file upload or download, including chunk progress, hashes, and retry counts.
//! Also provides helper functions for computing chunk layouts and byte offsets.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// Opaque identifier types (UUID v4 strings).
pub type SessionId = String;
pub type FileId = String;
pub type UserId = String;

/// File metadata associated with a transfer session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FileMeta {
    pub file_id: FileId,
    pub filename: String,
    pub size: u64,
    pub mime_type: Option<String>,
    pub chunk_size: usize,
    pub total_chunks: u64,
    pub whole_file_hash: String,
    pub hash_algorithm: String,
    pub uploaded_by: UserId,
    pub uploaded_at: DateTime<Utc>,
}

/// Direction of a file transfer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// Status of a transfer session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TransferStatus {
    InProgress,
    Paused,
    Completed,
    Failed { reason: String },
}

/// A stateful representation of an in-progress file upload or download.
///
/// Tracks chunk progress, per-chunk hashes, retry counts, and metadata.
/// Supports serialization for persistence to durable storage (Req 5.1).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransferSession {
    pub id: SessionId,
    pub file_id: FileId,
    pub user_id: UserId,
    pub direction: TransferDirection,
    pub file_meta: FileMeta,
    pub chunk_size: usize,
    pub total_chunks: u64,
    pub completed_chunks: BTreeSet<u64>,
    pub chunk_hashes: HashMap<u64, String>,
    pub status: TransferStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub retry_counts: HashMap<u64, u32>,
}

/// Compute the chunk layout for a file of given size.
///
/// Returns `(total_chunks, last_chunk_size)`.
///
/// # Panics
/// Panics if `chunk_size` is 0.
pub fn compute_chunk_layout(file_size: u64, chunk_size: usize) -> (u64, usize) {
    assert!(chunk_size > 0, "chunk_size must be > 0");
    if file_size == 0 {
        return (0, 0);
    }
    let cs = chunk_size as u64;
    let total = (file_size + cs - 1) / cs; // ceiling division
    let last = if file_size % cs == 0 {
        chunk_size
    } else {
        (file_size % cs) as usize
    };
    (total, last)
}

/// Compute the byte offset for a given chunk index.
pub fn chunk_offset(chunk_index: u64, chunk_size: usize) -> u64 {
    chunk_index * chunk_size as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_layout_basic() {
        // 10 bytes, chunk size 3 => 4 chunks, last chunk 1 byte
        let (total, last) = compute_chunk_layout(10, 3);
        assert_eq!(total, 4);
        assert_eq!(last, 1);
    }

    #[test]
    fn chunk_layout_exact_division() {
        // 12 bytes, chunk size 4 => 3 chunks, last chunk 4 bytes
        let (total, last) = compute_chunk_layout(12, 4);
        assert_eq!(total, 3);
        assert_eq!(last, 4);
    }

    #[test]
    fn chunk_layout_single_chunk() {
        let (total, last) = compute_chunk_layout(5, 10);
        assert_eq!(total, 1);
        assert_eq!(last, 5);
    }

    #[test]
    fn chunk_layout_zero_file() {
        let (total, last) = compute_chunk_layout(0, 4);
        assert_eq!(total, 0);
        assert_eq!(last, 0);
    }

    #[test]
    fn chunk_offset_basic() {
        assert_eq!(chunk_offset(0, 1024), 0);
        assert_eq!(chunk_offset(1, 1024), 1024);
        assert_eq!(chunk_offset(3, 256), 768);
    }

    #[test]
    fn session_round_trip_json() {
        let now = Utc::now();
        let session = TransferSession {
            id: "sess-1".to_string(),
            file_id: "file-1".to_string(),
            user_id: "user-1".to_string(),
            direction: TransferDirection::Upload,
            file_meta: FileMeta {
                file_id: "file-1".to_string(),
                filename: "test.txt".to_string(),
                size: 1024,
                mime_type: Some("text/plain".to_string()),
                chunk_size: 256,
                total_chunks: 4,
                whole_file_hash: "abc123".to_string(),
                hash_algorithm: "sha256".to_string(),
                uploaded_by: "user-1".to_string(),
                uploaded_at: now,
            },
            chunk_size: 256,
            total_chunks: 4,
            completed_chunks: BTreeSet::from([0, 2]),
            chunk_hashes: HashMap::from([
                (0, "hash0".to_string()),
                (2, "hash2".to_string()),
            ]),
            status: TransferStatus::InProgress,
            created_at: now,
            updated_at: now,
            retry_counts: HashMap::from([(1, 2)]),
        };

        let json = serde_json::to_string(&session).expect("serialize");
        let deserialized: TransferSession =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(session, deserialized);
    }
}
