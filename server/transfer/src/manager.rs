//! Transfer manager: orchestrates chunked uploads, downloads, resumption, and backpressure.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::rate_control::{BackpressureSignal, RateController, RateControllerConfig};
use crate::session::{
    chunk_offset, compute_chunk_layout, FileMeta, FileId, SessionId, TransferDirection,
    TransferSession, TransferStatus, UserId,
};
use integrity::{ChunkHash, HashAlgorithm, IntegrityError, IntegrityVerifier};
use storage::{StorageEngine, StorageError};

/// Errors produced by the TransferManager.
#[derive(Debug, Error)]
pub enum TransferError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Chunk failed for index {index}: {reason}")]
    ChunkFailed { index: u64, reason: String },
    #[error("Integrity error: {0}")]
    IntegrityError(#[from] IntegrityError),
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("Backpressure: {0}")]
    BackpressureError(#[from] BackpressureSignal),
    #[error("Source file changed since session was created")]
    SourceFileChanged,
    #[error("Max retries exceeded for chunk {index} (attempted {attempts} times)")]
    MaxRetriesExceeded { index: u64, attempts: u32 },
    #[error("Transfer not in progress")]
    NotInProgress,
    #[error("All chunks not yet completed")]
    IncompleteChunks,
}

/// Acknowledgement returned after initiating an upload.
#[derive(Debug, Clone)]
pub struct UploadAck {
    pub session_id: SessionId,
    pub chunk_size: usize,
    pub total_chunks: u64,
}

/// Acknowledgement returned after a chunk is received.
#[derive(Debug, Clone)]
pub struct ChunkAck {
    pub chunk_index: u64,
    pub session_id: SessionId,
}

/// Result returned after finalizing an upload.
#[derive(Debug, Clone)]
pub struct UploadComplete {
    pub session_id: SessionId,
    pub file_id: FileId,
    pub whole_file_hash: String,
}

/// Acknowledgement returned after initiating a download.
#[derive(Debug, Clone)]
pub struct DownloadAck {
    pub session_id: SessionId,
    pub file_id: FileId,
    pub file_size: u64,
    pub chunk_size: usize,
    pub total_chunks: u64,
}

/// Data for a single chunk (used in download responses).
#[derive(Debug, Clone)]
pub struct ChunkData {
    pub session_id: SessionId,
    pub chunk_index: u64,
    pub offset: u64,
    pub data: Vec<u8>,
    pub hash: ChunkHash,
}

/// Incoming chunk data for upload.
#[derive(Debug, Clone)]
pub struct IncomingChunk {
    pub chunk_index: u64,
    pub data: Vec<u8>,
    pub hash: ChunkHash,
}

/// Acknowledgement returned after resuming a transfer.
#[derive(Debug, Clone)]
pub struct ResumeAck {
    pub session_id: SessionId,
    pub first_incomplete_chunk: u64,
    pub completed_chunks: Vec<u64>,
    pub total_chunks: u64,
}

/// Result of resume validation.
#[derive(Debug)]
pub struct ResumeValidation {
    pub first_incomplete: u64,
}

/// Configuration for the TransferManager.
#[derive(Debug, Clone)]
pub struct TransferManagerConfig {
    pub chunk_size: usize,
    pub max_parallel_streams: usize,
    pub max_retries: u32,
    pub session_persist_path: PathBuf,
    pub backpressure_high_water: usize,
    pub backpressure_low_water: usize,
    pub per_session_rate_limit: u64,
    pub global_rate_limit: u64,
}

/// Orchestrates chunked uploads, downloads, resumption, and backpressure.
pub struct TransferManager {
    config: TransferManagerConfig,
    sessions: RwLock<HashMap<SessionId, TransferSession>>,
    rate_controller: RateController,
    storage: Arc<StorageEngine>,
    integrity: Arc<IntegrityVerifier>,
}

impl TransferManager {
    /// Create a new TransferManager.
    pub fn new(
        config: TransferManagerConfig,
        storage: Arc<StorageEngine>,
        integrity: Arc<IntegrityVerifier>,
    ) -> Self {
        let rate_controller = RateController::new(RateControllerConfig {
            per_session_limit: config.per_session_rate_limit,
            global_limit: config.global_rate_limit,
            high_water_mark: config.backpressure_high_water,
            low_water_mark: config.backpressure_low_water,
            memory_threshold: 0, // not used directly here
            max_parallelism: config.max_parallel_streams,
        });
        Self {
            config,
            sessions: RwLock::new(HashMap::new()),
            rate_controller,
            storage,
            integrity,
        }
    }

    // ── Upload Flow ──────────────────────────────────────────────────

    /// Initiate a new upload. Creates a session, computes chunk layout,
    /// allocates the file on disk, and returns an UploadAck (Req 3.1).
    pub async fn initiate_upload(
        &self,
        meta: FileMeta,
        user_id: UserId,
    ) -> Result<UploadAck, TransferError> {
        let (total_chunks, _last_chunk_size) =
            compute_chunk_layout(meta.size, self.config.chunk_size);

        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        // Pre-allocate the file on disk
        self.storage
            .allocate_file(meta.file_id.clone(), meta.size)
            .await?;

        let session = TransferSession {
            id: session_id.clone(),
            file_id: meta.file_id.clone(),
            user_id: user_id.clone(),
            direction: TransferDirection::Upload,
            file_meta: meta,
            chunk_size: self.config.chunk_size,
            total_chunks,
            completed_chunks: BTreeSet::new(),
            chunk_hashes: HashMap::new(),
            status: TransferStatus::InProgress,
            created_at: now,
            updated_at: now,
            retry_counts: HashMap::new(),
        };

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);

        Ok(UploadAck {
            session_id,
            chunk_size: self.config.chunk_size,
            total_chunks,
        })
    }

    /// Receive and process a single chunk (Req 3.3, 6.2).
    ///
    /// Checks backpressure, verifies chunk hash, writes to storage,
    /// and updates session state. Retries are tracked per-chunk.
    pub async fn receive_chunk(
        &self,
        session_id: SessionId,
        chunk: IncomingChunk,
    ) -> Result<ChunkAck, TransferError> {
        // Check backpressure via RateController
        self.rate_controller
            .acquire(&session_id, chunk.data.len())?;

        // Verify chunk hash via IntegrityVerifier (Req 6.2)
        self.integrity.verify_chunk(&chunk.data, &chunk.hash)?;

        // Compute offset and write via StorageEngine
        let file_id = {
            let sessions = self.sessions.read().await;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| TransferError::SessionNotFound(session_id.clone()))?;
            if session.status != TransferStatus::InProgress {
                return Err(TransferError::NotInProgress);
            }
            session.file_id.clone()
        };

        let offset = chunk_offset(chunk.chunk_index, self.config.chunk_size);
        let data_len = chunk.data.len();

        self.storage
            .write_chunk(file_id, offset, &chunk.data)
            .await?;

        // Report transferred bytes to rate controller
        self.rate_controller
            .report_transferred(&session_id, data_len);

        // Update session state
        {
            let mut sessions = self.sessions.write().await;
            let session = sessions
                .get_mut(&session_id)
                .ok_or_else(|| TransferError::SessionNotFound(session_id.clone()))?;
            session.completed_chunks.insert(chunk.chunk_index);
            session
                .chunk_hashes
                .insert(chunk.chunk_index, chunk.hash.value.clone());
            session.updated_at = Utc::now();
        }

        Ok(ChunkAck {
            chunk_index: chunk.chunk_index,
            session_id,
        })
    }

    /// Receive a chunk with retry logic. Retries up to max_retries on failure (Req 3.4).
    pub async fn receive_chunk_with_retry(
        &self,
        session_id: SessionId,
        chunk: IncomingChunk,
    ) -> Result<ChunkAck, TransferError> {
        let max_retries = self.config.max_retries;
        let chunk_index = chunk.chunk_index;

        // Check current retry count
        {
            let sessions = self.sessions.read().await;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| TransferError::SessionNotFound(session_id.clone()))?;
            let current_retries = session.retry_counts.get(&chunk_index).copied().unwrap_or(0);
            if current_retries >= max_retries {
                return Err(TransferError::MaxRetriesExceeded {
                    index: chunk_index,
                    attempts: current_retries + 1,
                });
            }
        }

        match self.receive_chunk(session_id.clone(), chunk).await {
            Ok(ack) => Ok(ack),
            Err(e) => {
                // Increment retry count
                let mut sessions = self.sessions.write().await;
                if let Some(session) = sessions.get_mut(&session_id) {
                    let count = session.retry_counts.entry(chunk_index).or_insert(0);
                    *count += 1;
                    if *count >= max_retries {
                        return Err(TransferError::MaxRetriesExceeded {
                            index: chunk_index,
                            attempts: *count + 1, // initial + retries
                        });
                    }
                }
                Err(e)
            }
        }
    }

    /// Finalize upload after all chunks received (Req 3.5, 6.4).
    ///
    /// Verifies whole-file hash by reading all chunks back and computing
    /// the aggregate hash. Marks session as complete on success.
    pub async fn finalize_upload(
        &self,
        session_id: SessionId,
    ) -> Result<UploadComplete, TransferError> {
        let (file_id, total_chunks, chunk_size, expected_hash, file_size) = {
            let sessions = self.sessions.read().await;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| TransferError::SessionNotFound(session_id.clone()))?;
            if session.status != TransferStatus::InProgress {
                return Err(TransferError::NotInProgress);
            }
            if session.completed_chunks.len() as u64 != session.total_chunks {
                return Err(TransferError::IncompleteChunks);
            }
            (
                session.file_id.clone(),
                session.total_chunks,
                session.chunk_size,
                session.file_meta.whole_file_hash.clone(),
                session.file_meta.size,
            )
        };

        // Read all chunks back and verify whole-file hash (Req 6.4)
        let mut all_data: Vec<Vec<u8>> = Vec::new();
        for i in 0..total_chunks {
            let offset = chunk_offset(i, chunk_size);
            let len = if i == total_chunks - 1 {
                let (_, last) = compute_chunk_layout(file_size, chunk_size);
                last
            } else {
                chunk_size
            };
            let data = self
                .storage
                .read_chunk(file_id.clone(), offset, len)
                .await?;
            all_data.push(data);
        }

        let chunk_slices: Vec<&[u8]> = all_data.iter().map(|d| d.as_slice()).collect();
        self.integrity
            .verify_file_from_chunks(chunk_slices.into_iter(), &expected_hash)?;

        // Mark session complete
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                session.status = TransferStatus::Completed;
                session.updated_at = Utc::now();
            }
        }

        Ok(UploadComplete {
            session_id,
            file_id,
            whole_file_hash: expected_hash,
        })
    }

    // ── Download Flow ────────────────────────────────────────────────

    /// Initiate a new download. Looks up file metadata, computes chunk layout,
    /// and returns a DownloadAck (Req 4.1).
    pub async fn initiate_download(
        &self,
        file_id: FileId,
        user_id: UserId,
    ) -> Result<DownloadAck, TransferError> {
        let file_meta = self.storage.get_file_meta(file_id.clone()).await?;
        let (total_chunks, _last_chunk_size) =
            compute_chunk_layout(file_meta.size, self.config.chunk_size);

        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        let session = TransferSession {
            id: session_id.clone(),
            file_id: file_id.clone(),
            user_id,
            direction: TransferDirection::Download,
            file_meta: FileMeta {
                file_id: file_id.clone(),
                filename: String::new(),
                size: file_meta.size,
                mime_type: None,
                chunk_size: self.config.chunk_size,
                total_chunks,
                whole_file_hash: String::new(),
                hash_algorithm: "sha256".to_string(),
                uploaded_by: String::new(),
                uploaded_at: now,
            },
            chunk_size: self.config.chunk_size,
            total_chunks,
            completed_chunks: BTreeSet::new(),
            chunk_hashes: HashMap::new(),
            status: TransferStatus::InProgress,
            created_at: now,
            updated_at: now,
            retry_counts: HashMap::new(),
        };

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);

        Ok(DownloadAck {
            session_id,
            file_id,
            file_size: file_meta.size,
            chunk_size: self.config.chunk_size,
            total_chunks,
        })
    }

    /// Read and send a chunk for download (Req 4.3).
    ///
    /// Reads from StorageEngine, computes chunk hash, applies rate control.
    pub async fn send_chunk(
        &self,
        session_id: SessionId,
        chunk_index: u64,
    ) -> Result<ChunkData, TransferError> {
        let (file_id, chunk_size, total_chunks, file_size) = {
            let sessions = self.sessions.read().await;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| TransferError::SessionNotFound(session_id.clone()))?;
            if session.status != TransferStatus::InProgress {
                return Err(TransferError::NotInProgress);
            }
            (
                session.file_id.clone(),
                session.chunk_size,
                session.total_chunks,
                session.file_meta.size,
            )
        };

        let offset = chunk_offset(chunk_index, chunk_size);
        let len = if chunk_index == total_chunks - 1 {
            let (_, last) = compute_chunk_layout(file_size, chunk_size);
            last
        } else {
            chunk_size
        };

        // Apply rate control
        self.rate_controller.acquire(&session_id, len)?;

        // Read from storage
        let data = self
            .storage
            .read_chunk(file_id, offset, len)
            .await?;

        // Compute chunk hash
        let hash = self
            .integrity
            .hash_chunk(&data, HashAlgorithm::Sha256);

        // Report transferred bytes
        self.rate_controller
            .report_transferred(&session_id, data.len());

        // Update session state
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                session.completed_chunks.insert(chunk_index);
                session
                    .chunk_hashes
                    .insert(chunk_index, hash.value.clone());
                session.updated_at = Utc::now();
            }
        }

        Ok(ChunkData {
            session_id,
            chunk_index,
            offset,
            data,
            hash,
        })
    }

    // ── Resumable Transfer Logic ─────────────────────────────────────

    /// Resume a previously interrupted transfer (Req 5.2, 5.3).
    ///
    /// Loads the persisted session, validates the source file hasn't changed,
    /// verifies completed chunk hashes, and resumes from the first incomplete chunk.
    pub async fn resume_transfer(
        &self,
        session_id: SessionId,
        current_file_size: u64,
        current_modified: DateTime<Utc>,
    ) -> Result<ResumeAck, TransferError> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(&session_id)
                .ok_or_else(|| TransferError::SessionNotFound(session_id.clone()))?
                .clone()
        };

        // Validate resume conditions
        let validation = validate_resume(
            &session,
            current_file_size,
            current_modified,
            &self.storage,
            &self.integrity,
        )
        .await?;

        // Re-activate the session as InProgress
        {
            let mut sessions = self.sessions.write().await;
            if let Some(s) = sessions.get_mut(&session_id) {
                s.status = TransferStatus::InProgress;
                s.updated_at = Utc::now();
            }
        }

        let completed: Vec<u64> = session.completed_chunks.iter().copied().collect();

        Ok(ResumeAck {
            session_id,
            first_incomplete_chunk: validation.first_incomplete,
            completed_chunks: completed,
            total_chunks: session.total_chunks,
        })
    }

    // ── Session helpers ──────────────────────────────────────────────

    /// Get a clone of a session by ID.
    pub async fn get_session(&self, session_id: &str) -> Option<TransferSession> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Insert a session directly (useful for testing / persistence restore).
    pub async fn insert_session(&self, session: TransferSession) {
        self.sessions
            .write()
            .await
            .insert(session.id.clone(), session);
    }
}

/// Validate that a transfer can be resumed (Req 5.3, 5.4).
///
/// Checks that the source file hasn't changed and that completed chunk hashes
/// still match the data on disk.
pub async fn validate_resume(
    session: &TransferSession,
    current_file_size: u64,
    _current_modified: DateTime<Utc>,
    storage: &StorageEngine,
    verifier: &IntegrityVerifier,
) -> Result<ResumeValidation, TransferError> {
    // Check file hasn't changed (Req 5.4)
    if session.file_meta.size != current_file_size {
        return Err(TransferError::SourceFileChanged);
    }

    // Verify completed chunk hashes still match (Req 5.3)
    for (&index, expected_hash_value) in &session.chunk_hashes {
        let offset = chunk_offset(index, session.chunk_size);
        let total = session.total_chunks;
        let len = if index == total - 1 {
            let (_, last) = compute_chunk_layout(session.file_meta.size, session.chunk_size);
            last
        } else {
            session.chunk_size
        };

        let data = storage
            .read_chunk(session.file_id.clone(), offset, len)
            .await?;

        let expected = ChunkHash {
            algorithm: HashAlgorithm::Sha256,
            value: expected_hash_value.clone(),
        };
        verifier.verify_chunk(&data, &expected)?;
    }

    // Find first incomplete chunk (Req 5.2)
    let first_incomplete = (0..session.total_chunks)
        .find(|i| !session.completed_chunks.contains(i))
        .unwrap_or(session.total_chunks);

    Ok(ResumeValidation {
        first_incomplete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::StorageEngineConfig;
    use tempfile::TempDir;

    fn test_config(tmp: &std::path::Path) -> TransferManagerConfig {
        TransferManagerConfig {
            chunk_size: 4,
            max_parallel_streams: 4,
            max_retries: 3,
            session_persist_path: tmp.to_path_buf(),
            backpressure_high_water: 100_000,
            backpressure_low_water: 50_000,
            per_session_rate_limit: 1_000_000,
            global_rate_limit: 10_000_000,
        }
    }

    fn test_storage(tmp: &std::path::Path) -> Arc<StorageEngine> {
        Arc::new(StorageEngine::new(StorageEngineConfig {
            data_dir: tmp.to_path_buf(),
            max_concurrent_writes: 4,
            write_buffer_size: 4096,
        }))
    }

    fn test_integrity() -> Arc<IntegrityVerifier> {
        Arc::new(IntegrityVerifier::new(HashAlgorithm::Sha256))
    }

    fn test_file_meta(file_id: &str, size: u64, hash: &str) -> FileMeta {
        FileMeta {
            file_id: file_id.to_string(),
            filename: "test.bin".to_string(),
            size,
            mime_type: None,
            chunk_size: 4,
            total_chunks: 0, // will be computed
            whole_file_hash: hash.to_string(),
            hash_algorithm: "sha256".to_string(),
            uploaded_by: "user1".to_string(),
            uploaded_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn initiate_upload_creates_session() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(tmp.path());
        let integrity = test_integrity();
        let config = test_config(tmp.path());
        let tm = TransferManager::new(config, storage, integrity);

        let meta = test_file_meta("file1", 10, "somehash");
        let ack = tm
            .initiate_upload(meta, "user1".to_string())
            .await
            .unwrap();

        assert_eq!(ack.chunk_size, 4);
        assert_eq!(ack.total_chunks, 3); // ceil(10/4) = 3
        assert!(tm.get_session(&ack.session_id).await.is_some());
    }

    #[tokio::test]
    async fn upload_and_finalize() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(tmp.path());
        let integrity = test_integrity();
        let config = test_config(tmp.path());
        let tm = TransferManager::new(config, storage.clone(), integrity.clone());

        // File data: 8 bytes => 2 chunks of 4
        let file_data = b"abcdefgh";
        let verifier = IntegrityVerifier::new(HashAlgorithm::Sha256);
        let whole_hash = verifier.hash_chunk(file_data, HashAlgorithm::Sha256);

        let meta = test_file_meta("file2", 8, &whole_hash.value);
        let ack = tm
            .initiate_upload(meta, "user1".to_string())
            .await
            .unwrap();
        assert_eq!(ack.total_chunks, 2);

        // Send chunk 0
        let hash0 = verifier.hash_chunk(&file_data[0..4], HashAlgorithm::Sha256);
        tm.receive_chunk(
            ack.session_id.clone(),
            IncomingChunk {
                chunk_index: 0,
                data: file_data[0..4].to_vec(),
                hash: hash0,
            },
        )
        .await
        .unwrap();

        // Send chunk 1
        let hash1 = verifier.hash_chunk(&file_data[4..8], HashAlgorithm::Sha256);
        tm.receive_chunk(
            ack.session_id.clone(),
            IncomingChunk {
                chunk_index: 1,
                data: file_data[4..8].to_vec(),
                hash: hash1,
            },
        )
        .await
        .unwrap();

        // Finalize
        let complete = tm.finalize_upload(ack.session_id.clone()).await.unwrap();
        assert_eq!(complete.whole_file_hash, whole_hash.value);

        let session = tm.get_session(&ack.session_id).await.unwrap();
        assert_eq!(session.status, TransferStatus::Completed);
    }

    #[tokio::test]
    async fn receive_chunk_bad_hash_fails() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(tmp.path());
        let integrity = test_integrity();
        let config = test_config(tmp.path());
        let tm = TransferManager::new(config, storage, integrity);

        let meta = test_file_meta("file3", 8, "hash");
        let ack = tm
            .initiate_upload(meta, "user1".to_string())
            .await
            .unwrap();

        let bad_hash = ChunkHash {
            algorithm: HashAlgorithm::Sha256,
            value: "badhash".to_string(),
        };
        let result = tm
            .receive_chunk(
                ack.session_id.clone(),
                IncomingChunk {
                    chunk_index: 0,
                    data: vec![1, 2, 3, 4],
                    hash: bad_hash,
                },
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn download_flow() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(tmp.path());
        let integrity = test_integrity();
        let config = test_config(tmp.path());
        let tm = TransferManager::new(config, storage.clone(), integrity);

        // Pre-create a file
        let file_data = b"12345678";
        storage
            .allocate_file("dl_file".to_string(), 8)
            .await
            .unwrap();
        storage
            .write_chunk("dl_file".to_string(), 0, file_data)
            .await
            .unwrap();

        let ack = tm
            .initiate_download("dl_file".to_string(), "user1".to_string())
            .await
            .unwrap();
        assert_eq!(ack.total_chunks, 2);
        assert_eq!(ack.file_size, 8);

        // Read chunk 0
        let chunk0 = tm.send_chunk(ack.session_id.clone(), 0).await.unwrap();
        assert_eq!(chunk0.data, &file_data[0..4]);

        // Read chunk 1
        let chunk1 = tm.send_chunk(ack.session_id.clone(), 1).await.unwrap();
        assert_eq!(chunk1.data, &file_data[4..8]);
    }

    #[tokio::test]
    async fn resume_detects_file_change() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(tmp.path());
        let integrity = test_integrity();
        let config = test_config(tmp.path());
        let tm = TransferManager::new(config, storage, integrity);

        let meta = test_file_meta("file_resume", 8, "hash");
        let ack = tm
            .initiate_upload(meta, "user1".to_string())
            .await
            .unwrap();

        // Try to resume with different file size
        let result = tm
            .resume_transfer(ack.session_id.clone(), 16, Utc::now())
            .await;
        assert!(matches!(result, Err(TransferError::SourceFileChanged)));
    }

    #[tokio::test]
    async fn resume_finds_first_incomplete() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(tmp.path());
        let integrity = test_integrity();
        let config = test_config(tmp.path());
        let tm = TransferManager::new(config, storage.clone(), integrity.clone());

        // Create file and upload some chunks
        let file_data = b"aabbccdd";
        let verifier = IntegrityVerifier::new(HashAlgorithm::Sha256);

        let meta = test_file_meta("file_resume2", 8, "hash");
        let ack = tm
            .initiate_upload(meta, "user1".to_string())
            .await
            .unwrap();

        // Complete chunk 0 only
        let hash0 = verifier.hash_chunk(&file_data[0..4], HashAlgorithm::Sha256);
        tm.receive_chunk(
            ack.session_id.clone(),
            IncomingChunk {
                chunk_index: 0,
                data: file_data[0..4].to_vec(),
                hash: hash0,
            },
        )
        .await
        .unwrap();

        // Resume should start from chunk 1
        let resume = tm
            .resume_transfer(ack.session_id.clone(), 8, Utc::now())
            .await
            .unwrap();
        assert_eq!(resume.first_incomplete_chunk, 1);
        assert_eq!(resume.completed_chunks, vec![0]);
    }
}
