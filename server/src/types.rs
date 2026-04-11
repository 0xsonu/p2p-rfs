use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Opaque identifiers (UUID v4 strings)
pub type SessionId = String;
pub type FileId = String;
pub type UserId = String;

/// File metadata stored in the metadata database.
#[derive(Clone, Debug, Serialize, Deserialize)]
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

/// Session token issued after authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionToken {
    pub token: String,
    pub user_id: UserId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// User record in the credential store.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRecord {
    pub user_id: UserId,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

/// Transfer history entry for UI display.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferHistoryEntry {
    pub session_id: SessionId,
    pub file_id: FileId,
    pub filename: String,
    pub direction: TransferDirection,
    pub file_size: u64,
    pub status: TransferStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub avg_throughput_bps: Option<f64>,
}

/// Settings persisted on the client side.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientSettings {
    pub chunk_size: usize,
    pub parallel_streams: usize,
    pub per_transfer_rate_limit: u64,
    pub server_address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TransferDirection {
    Upload,
    Download,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TransferStatus {
    InProgress,
    Paused,
    Completed,
    Failed { reason: String },
}
