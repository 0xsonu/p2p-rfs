//! Tauri event emission helpers for the P2P engine.
//!
//! Defines event name constants, payload structs, and `emit_*` helper functions
//! that call `app_handle.emit()` to push real-time updates to the React UI.

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::peer_registry::PeerInfo;

// ---------------------------------------------------------------------------
// Event name constants
// ---------------------------------------------------------------------------

/// A new peer was discovered on the local network via mDNS.
pub const EVENT_PEER_DISCOVERED: &str = "peer-discovered";

/// A previously discovered peer is no longer reachable.
pub const EVENT_PEER_LOST: &str = "peer-lost";

/// A remote peer wants to send a file to this instance.
pub const EVENT_INCOMING_TRANSFER: &str = "incoming-transfer-request";

/// Chunk-level progress update for an active transfer.
pub const EVENT_TRANSFER_PROGRESS: &str = "transfer-progress";

/// A transfer completed successfully.
pub const EVENT_TRANSFER_COMPLETE: &str = "transfer-complete";

/// A transfer failed.
pub const EVENT_TRANSFER_FAILED: &str = "transfer-failed";

// ---------------------------------------------------------------------------
// Payload structs
// ---------------------------------------------------------------------------

/// Payload emitted with [`EVENT_PEER_LOST`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PeerLostPayload {
    pub peer_id: String,
}

/// Payload emitted with [`EVENT_INCOMING_TRANSFER`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IncomingTransferPayload {
    pub session_id: String,
    pub sender_name: String,
    pub file_name: String,
    pub file_size: u64,
    pub whole_file_hash: String,
}

/// Payload emitted with [`EVENT_TRANSFER_PROGRESS`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransferProgressPayload {
    pub session_id: String,
    pub file_name: String,
    pub direction: String,
    pub completed_chunks: u64,
    pub total_chunks: u64,
    pub percentage: f64,
    pub speed_bps: f64,
    pub eta_seconds: f64,
}

/// Payload emitted with [`EVENT_TRANSFER_COMPLETE`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransferCompletePayload {
    pub session_id: String,
    pub file_name: String,
    pub hash: String,
}

/// Payload emitted with [`EVENT_TRANSFER_FAILED`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransferFailedPayload {
    pub session_id: String,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Emit helper functions
// ---------------------------------------------------------------------------

/// Emit a [`EVENT_PEER_DISCOVERED`] event with the full [`PeerInfo`].
pub fn emit_peer_discovered(app_handle: &tauri::AppHandle, peer: &PeerInfo) {
    if let Err(e) = app_handle.emit(EVENT_PEER_DISCOVERED, peer.clone()) {
        tracing::warn!(error = %e, "Failed to emit peer-discovered event");
    }
}

/// Emit a [`EVENT_PEER_LOST`] event with the peer ID.
pub fn emit_peer_lost(app_handle: &tauri::AppHandle, peer_id: &str) {
    let payload = PeerLostPayload {
        peer_id: peer_id.to_string(),
    };
    if let Err(e) = app_handle.emit(EVENT_PEER_LOST, payload) {
        tracing::warn!(error = %e, "Failed to emit peer-lost event");
    }
}

/// Emit a [`EVENT_INCOMING_TRANSFER`] event when a remote peer requests to send a file.
pub fn emit_incoming_transfer(app_handle: &tauri::AppHandle, payload: &IncomingTransferPayload) {
    if let Err(e) = app_handle.emit(EVENT_INCOMING_TRANSFER, payload.clone()) {
        tracing::warn!(error = %e, "Failed to emit incoming-transfer-request event");
    }
}

/// Emit a [`EVENT_TRANSFER_PROGRESS`] event with chunk-level progress data.
pub fn emit_transfer_progress(app_handle: &tauri::AppHandle, payload: &TransferProgressPayload) {
    if let Err(e) = app_handle.emit(EVENT_TRANSFER_PROGRESS, payload.clone()) {
        tracing::warn!(error = %e, "Failed to emit transfer-progress event");
    }
}

/// Emit a [`EVENT_TRANSFER_COMPLETE`] event when a transfer finishes successfully.
pub fn emit_transfer_complete(app_handle: &tauri::AppHandle, payload: &TransferCompletePayload) {
    if let Err(e) = app_handle.emit(EVENT_TRANSFER_COMPLETE, payload.clone()) {
        tracing::warn!(error = %e, "Failed to emit transfer-complete event");
    }
}

/// Emit a [`EVENT_TRANSFER_FAILED`] event when a transfer fails.
pub fn emit_transfer_failed(app_handle: &tauri::AppHandle, payload: &TransferFailedPayload) {
    if let Err(e) = app_handle.emit(EVENT_TRANSFER_FAILED, payload.clone()) {
        tracing::warn!(error = %e, "Failed to emit transfer-failed event");
    }
}
