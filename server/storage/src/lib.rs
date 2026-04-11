//! Storage engine: direct-offset file writes and reads.

use std::path::PathBuf;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Semaphore;

/// Opaque file identifier (UUID v4 string).
pub type FileId = String;

/// Basic file metadata returned by the storage engine.
#[derive(Debug, Clone)]
pub struct FileMeta {
    pub file_id: FileId,
    pub size: u64,
}

/// Configuration for the storage engine.
#[derive(Debug, Clone)]
pub struct StorageEngineConfig {
    pub data_dir: PathBuf,
    pub max_concurrent_writes: usize,
    pub write_buffer_size: usize,
}

/// Errors produced by the storage engine.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Disk write failed at offset {offset}: {reason}")]
    WriteFailed { offset: u64, reason: String },
    #[error("File not found: {file_id}")]
    FileNotFound { file_id: String },
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Direct-offset storage engine backed by the local filesystem.
///
/// Uses a semaphore to cap concurrent write operations (Req 9.3)
/// and performs seek-to-offset writes so chunks can arrive out of
/// order without buffering the whole file (Req 9.1, 9.2).
pub struct StorageEngine {
    config: StorageEngineConfig,
    write_semaphore: Semaphore,
}

impl StorageEngine {
    /// Create a new `StorageEngine` from the given config.
    pub fn new(config: StorageEngineConfig) -> Self {
        let write_semaphore = Semaphore::new(config.max_concurrent_writes);
        Self {
            config,
            write_semaphore,
        }
    }

    /// Resolve a `FileId` to its path on disk.
    fn file_path(&self, file_id: &str) -> PathBuf {
        self.config.data_dir.join(file_id)
    }

    /// Write `data` at the given byte `offset` inside the file identified by
    /// `file_id`.  Acquires a semaphore permit to enforce the concurrent-write
    /// cap (Req 9.3).
    pub async fn write_chunk(
        &self,
        file_id: FileId,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let _permit = self.write_semaphore.acquire().await.map_err(|e| {
            StorageError::WriteFailed {
                offset,
                reason: format!("semaphore closed: {e}"),
            }
        })?;

        let path = self.file_path(&file_id);
        if !path.exists() {
            return Err(StorageError::FileNotFound { file_id });
        }

        let mut file = OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .map_err(|e| StorageError::WriteFailed {
                offset,
                reason: e.to_string(),
            })?;

        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| StorageError::WriteFailed {
                offset,
                reason: e.to_string(),
            })?;

        file.write_all(data).await.map_err(|e| StorageError::WriteFailed {
            offset,
            reason: e.to_string(),
        })?;

        file.flush().await.map_err(|e| StorageError::WriteFailed {
            offset,
            reason: e.to_string(),
        })?;

        Ok(())
    }

    /// Read `length` bytes starting at `offset` from the file identified by
    /// `file_id`.
    pub async fn read_chunk(
        &self,
        file_id: FileId,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, StorageError> {
        let path = self.file_path(&file_id);
        if !path.exists() {
            return Err(StorageError::FileNotFound { file_id });
        }

        let mut file = File::open(&path).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;

        let mut buf = vec![0u8; length];
        file.read_exact(&mut buf).await?;
        Ok(buf)
    }

    /// Return basic metadata (size) for the given file.
    pub async fn get_file_meta(&self, file_id: FileId) -> Result<FileMeta, StorageError> {
        let path = self.file_path(&file_id);
        if !path.exists() {
            return Err(StorageError::FileNotFound { file_id });
        }

        let metadata = fs::metadata(&path).await?;
        Ok(FileMeta {
            file_id,
            size: metadata.len(),
        })
    }

    /// Pre-allocate a file of the given `size` on disk.
    pub async fn allocate_file(
        &self,
        file_id: FileId,
        size: u64,
    ) -> Result<(), StorageError> {
        let path = self.file_path(&file_id);
        fs::create_dir_all(&self.config.data_dir).await?;
        let file = File::create(&path).await?;
        file.set_len(size).await?;
        Ok(())
    }
}
