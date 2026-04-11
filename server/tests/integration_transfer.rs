//! Integration tests for core transfer flows.
//!
//! These tests wire together the actual Rust components (TransferManager,
//! StorageEngine, IntegrityVerifier) without QUIC transport,
//! using direct function calls to validate end-to-end correctness.
//!
//! Requirements: 21.3

use std::sync::Arc;

use chrono::Utc;
use integrity::{HashAlgorithm, IntegrityVerifier};
use storage::{StorageEngine, StorageEngineConfig};
use tempfile::TempDir;
use transfer::manager::{
    IncomingChunk, TransferManager, TransferManagerConfig,
};
use transfer::session::{FileMeta, TransferStatus};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_transfer_manager(tmp: &std::path::Path) -> (TransferManager, Arc<StorageEngine>, Arc<IntegrityVerifier>) {
    let storage = Arc::new(StorageEngine::new(StorageEngineConfig {
        data_dir: tmp.to_path_buf(),
        max_concurrent_writes: 4,
        write_buffer_size: 4096,
    }));
    let integrity = Arc::new(IntegrityVerifier::new(HashAlgorithm::Sha256));
    let config = TransferManagerConfig {
        chunk_size: 4,
        max_parallel_streams: 4,
        max_retries: 3,
        session_persist_path: tmp.to_path_buf(),
        backpressure_high_water: 100_000,
        backpressure_low_water: 50_000,
        per_session_rate_limit: 10_000_000,
        global_rate_limit: 100_000_000,
    };
    let tm = TransferManager::new(config, storage.clone(), integrity.clone());
    (tm, storage, integrity)
}

fn make_file_meta(file_id: &str, data: &[u8], verifier: &IntegrityVerifier) -> FileMeta {
    let whole_hash = verifier.hash_chunk(data, HashAlgorithm::Sha256);
    FileMeta {
        file_id: file_id.to_string(),
        filename: "test.bin".to_string(),
        size: data.len() as u64,
        mime_type: None,
        chunk_size: 4,
        total_chunks: 0,
        whole_file_hash: whole_hash.value,
        hash_algorithm: "sha256".to_string(),
        uploaded_by: "user1".to_string(),
        uploaded_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Test: Full upload flow
// file → chunks → parallel upload → integrity verify → complete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_upload_flow() {
    let tmp = TempDir::new().unwrap();
    let (tm, _storage, integrity) = make_transfer_manager(tmp.path());

    let file_data = b"Hello, World! This is a test file for upload.";
    let meta = make_file_meta("upload_file", file_data, &integrity);

    // 1. Initiate upload
    let ack = tm.initiate_upload(meta, "user1".to_string()).await.unwrap();
    assert!(ack.total_chunks > 0);
    assert_eq!(ack.chunk_size, 4);

    // 2. Send all chunks
    let chunk_size = ack.chunk_size;
    for i in 0..ack.total_chunks {
        let start = (i as usize) * chunk_size;
        let end = std::cmp::min(start + chunk_size, file_data.len());
        let chunk_data = &file_data[start..end];
        let hash = integrity.hash_chunk(chunk_data, HashAlgorithm::Sha256);

        let chunk_ack = tm
            .receive_chunk(
                ack.session_id.clone(),
                IncomingChunk {
                    chunk_index: i,
                    data: chunk_data.to_vec(),
                    hash,
                },
            )
            .await
            .unwrap();
        assert_eq!(chunk_ack.chunk_index, i);
    }

    // 3. Finalize upload — verifies whole-file integrity
    let complete = tm.finalize_upload(ack.session_id.clone()).await.unwrap();
    assert_eq!(complete.file_id, "upload_file");
    assert!(!complete.whole_file_hash.is_empty());

    // 4. Verify session is marked complete
    let session = tm.get_session(&ack.session_id).await.unwrap();
    assert_eq!(session.status, TransferStatus::Completed);
    assert_eq!(session.completed_chunks.len() as u64, ack.total_chunks);
}

// ---------------------------------------------------------------------------
// Test: Full download flow
// request → parallel download → verify → assemble
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_download_flow() {
    let tmp = TempDir::new().unwrap();
    let (tm, storage, integrity) = make_transfer_manager(tmp.path());

    // Pre-create a file on disk
    let file_data = b"Download me! This is test content.";
    storage
        .allocate_file("dl_file".to_string(), file_data.len() as u64)
        .await
        .unwrap();
    storage
        .write_chunk("dl_file".to_string(), 0, file_data)
        .await
        .unwrap();

    // 1. Initiate download
    let ack = tm
        .initiate_download("dl_file".to_string(), "user1".to_string())
        .await
        .unwrap();
    assert_eq!(ack.file_size, file_data.len() as u64);
    assert!(ack.total_chunks > 0);

    // 2. Download all chunks and verify each
    let mut assembled = Vec::new();
    for i in 0..ack.total_chunks {
        let chunk = tm.send_chunk(ack.session_id.clone(), i).await.unwrap();

        // Verify chunk hash
        integrity.verify_chunk(&chunk.data, &chunk.hash).unwrap();

        assembled.extend_from_slice(&chunk.data);
    }

    // 3. Verify assembled file matches original
    assert_eq!(&assembled[..file_data.len()], file_data);

    // 4. Verify whole-file integrity
    let whole_hash = integrity.hash_chunk(file_data, HashAlgorithm::Sha256);
    let chunk_slices: Vec<&[u8]> = vec![file_data.as_slice()];
    integrity
        .verify_file_from_chunks(chunk_slices.into_iter(), &whole_hash.value)
        .unwrap();
}

// ---------------------------------------------------------------------------
// Test: Resume flow
// partial upload → disconnect → reconnect → resume → complete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_resume_flow() {
    let tmp = TempDir::new().unwrap();
    let (tm, _storage, integrity) = make_transfer_manager(tmp.path());

    let file_data = b"Resume test data here!!";
    let meta = make_file_meta("resume_file", file_data, &integrity);
    let file_size = file_data.len() as u64;

    // 1. Initiate upload
    let ack = tm.initiate_upload(meta, "user1".to_string()).await.unwrap();
    let total = ack.total_chunks;
    assert!(total > 1, "need multiple chunks for resume test");

    // 2. Upload only the first chunk (simulate partial upload)
    let first_chunk = &file_data[0..4];
    let hash0 = integrity.hash_chunk(first_chunk, HashAlgorithm::Sha256);
    tm.receive_chunk(
        ack.session_id.clone(),
        IncomingChunk {
            chunk_index: 0,
            data: first_chunk.to_vec(),
            hash: hash0,
        },
    )
    .await
    .unwrap();

    // 3. "Disconnect" — session persists in memory (simulating persistence)

    // 4. Resume transfer — should start from chunk 1
    let resume = tm
        .resume_transfer(ack.session_id.clone(), file_size, Utc::now())
        .await
        .unwrap();
    assert_eq!(resume.first_incomplete_chunk, 1);
    assert_eq!(resume.completed_chunks, vec![0]);
    assert_eq!(resume.total_chunks, total);

    // 5. Upload remaining chunks
    for i in 1..total {
        let start = (i as usize) * 4;
        let end = std::cmp::min(start + 4, file_data.len());
        let chunk_data = &file_data[start..end];
        let hash = integrity.hash_chunk(chunk_data, HashAlgorithm::Sha256);

        tm.receive_chunk(
            ack.session_id.clone(),
            IncomingChunk {
                chunk_index: i,
                data: chunk_data.to_vec(),
                hash,
            },
        )
        .await
        .unwrap();
    }

    // 6. Finalize
    let complete = tm.finalize_upload(ack.session_id.clone()).await.unwrap();
    assert!(!complete.whole_file_hash.is_empty());

    let session = tm.get_session(&ack.session_id).await.unwrap();
    assert_eq!(session.status, TransferStatus::Completed);
}
