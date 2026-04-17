use std::net::SocketAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::p2p_engine::{P2PError, P2PTransferDirection, P2PTransferSession, P2PTransferStatus};
use crate::peer_registry::PeerInfo;
use crate::settings::{self, P2PConfig, P2PSettings, SettingsError};
use crate::state::AppState;

/// Structured error returned to the React UI from every Tauri command.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<P2PError> for CommandError {
    fn from(err: P2PError) -> Self {
        match &err {
            P2PError::Transport(_) => CommandError {
                code: "TRANSPORT_ERROR".into(),
                message: err.to_string(),
            },
            P2PError::Cert(_) => CommandError {
                code: "CERT_ERROR".into(),
                message: err.to_string(),
            },
            P2PError::Discovery(_) => CommandError {
                code: "DISCOVERY_ERROR".into(),
                message: err.to_string(),
            },
            P2PError::Protocol(_) => CommandError {
                code: "PROTOCOL_ERROR".into(),
                message: err.to_string(),
            },
            P2PError::PeerNotFound(_) => CommandError {
                code: "PEER_NOT_FOUND".into(),
                message: err.to_string(),
            },
            P2PError::NotRunning => CommandError {
                code: "ENGINE_NOT_RUNNING".into(),
                message: "P2P engine is not running.".into(),
            },
            P2PError::AlreadyRunning => CommandError {
                code: "ENGINE_ALREADY_RUNNING".into(),
                message: "P2P engine is already running.".into(),
            },
            P2PError::BindFailed => CommandError {
                code: "BIND_FAILED".into(),
                message: "Failed to bind QUIC listener after exhausting port range.".into(),
            },
            P2PError::Transfer(_) => CommandError {
                code: "TRANSFER_ERROR".into(),
                message: err.to_string(),
            },
            P2PError::SessionNotFound(_) => CommandError {
                code: "SESSION_NOT_FOUND".into(),
                message: err.to_string(),
            },
            P2PError::TransferRejected(_) => CommandError {
                code: "TRANSFER_REJECTED".into(),
                message: err.to_string(),
            },
            P2PError::Integrity(_) => CommandError {
                code: "INTEGRITY_ERROR".into(),
                message: err.to_string(),
            },
            P2PError::Storage(_) => CommandError {
                code: "STORAGE_ERROR".into(),
                message: err.to_string(),
            },
            P2PError::Io(_) => CommandError {
                code: "IO_ERROR".into(),
                message: err.to_string(),
            },
            P2PError::SourceFileChanged => CommandError {
                code: "FILE_CHANGED".into(),
                message: "Source file changed. Please restart the transfer.".into(),
            },
        }
    }
}

impl From<SettingsError> for CommandError {
    fn from(err: SettingsError) -> Self {
        match &err {
            SettingsError::ValidationFailed { .. } => CommandError {
                code: "VALIDATION_ERROR".into(),
                message: err.to_string(),
            },
            SettingsError::IoError(_) => CommandError {
                code: "SETTINGS_IO_ERROR".into(),
                message: err.to_string(),
            },
        }
    }
}

/// Information returned after the P2P engine starts successfully.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EngineInfo {
    pub bound_port: u16,
    pub fingerprint: String,
    pub display_name: String,
}

/// A single entry in the transfer history list.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TransferHistoryEntry {
    pub session_id: String,
    pub file_name: String,
    pub direction: String,
    pub peer_display_name: String,
    pub timestamp: String,
    pub file_size: u64,
    pub status: String,
    pub failure_reason: Option<String>,
}

/// Serializable wrapper for LocalPeerInfo to return from commands.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LocalInfo {
    pub display_name: String,
    pub listen_port: u16,
    pub cert_fingerprint: String,
}

// ---------------------------------------------------------------------------
// Tauri Commands
// ---------------------------------------------------------------------------

/// Start the P2P engine. Creates a P2PConfig from current settings and
/// initializes all subsystems (certs, QUIC listener, mDNS discovery).
/// If the engine is already running, returns its info instead of erroring.
#[tauri::command]
pub async fn start_engine(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<EngineInfo, CommandError> {
    // If already running, return existing engine info.
    {
        let engine_guard = state.engine.read().await;
        if let Some(engine) = engine_guard.as_ref() {
            return Ok(EngineInfo {
                bound_port: engine.bound_port(),
                fingerprint: engine.cert_manager().fingerprint().to_string(),
                display_name: engine.config().display_name.clone(),
            });
        }
    }

    let mut engine_guard = state.engine.write().await;
    // Double-check after acquiring write lock (another task may have started it).
    if let Some(engine) = engine_guard.as_ref() {
        return Ok(EngineInfo {
            bound_port: engine.bound_port(),
            fingerprint: engine.cert_manager().fingerprint().to_string(),
            display_name: engine.config().display_name.clone(),
        });
    }

    let current_settings = state.settings.read().await.clone();

    // Resolve the data directory from the app handle.
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));

    let config = P2PConfig::from_settings(&current_settings, data_dir);

    let engine = match crate::p2p_engine::P2PEngine::start(config).await {
        Ok(engine) => engine,
        Err(e) => {
            tracing::error!(error = %e, "P2P engine failed to start");
            return Err(CommandError::from(e));
        }
    };

    engine.set_app_handle(app_handle).await;

    let info = EngineInfo {
        bound_port: engine.bound_port(),
        fingerprint: engine.cert_manager().fingerprint().to_string(),
        display_name: engine.config().display_name.clone(),
    };

    *engine_guard = Some(engine);
    Ok(info)
}

/// Gracefully stop the P2P engine.
#[tauri::command]
pub async fn stop_engine(state: State<'_, AppState>) -> Result<(), CommandError> {
    let mut engine_guard = state.engine.write().await;
    match engine_guard.take() {
        Some(engine) => {
            engine.shutdown().await.map_err(CommandError::from)?;
            Ok(())
        }
        None => Err(P2PError::NotRunning.into()),
    }
}

/// List all currently discovered peers from the peer registry.
#[tauri::command]
pub async fn list_peers(state: State<'_, AppState>) -> Result<Vec<PeerInfo>, CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;
    Ok(engine.peer_registry().list())
}

/// Connect to a peer by address string (e.g. "192.168.1.10:4433").
#[tauri::command]
pub async fn connect_to_peer(
    state: State<'_, AppState>,
    address: String,
) -> Result<PeerInfo, CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    let addr: SocketAddr = address.parse().map_err(|e| CommandError {
        code: "INVALID_ADDRESS".into(),
        message: format!("Invalid address: {e}"),
    })?;

    let peer_id = engine
        .connect_to_peer(addr)
        .await
        .map_err(CommandError::from)?;

    let peer_info = engine
        .peer_registry()
        .get(&peer_id)
        .ok_or_else(|| CommandError {
            code: "PEER_NOT_FOUND".into(),
            message: format!("Peer {peer_id} not found in registry after connection"),
        })?;

    Ok(peer_info)
}

/// Initiate sending a file to a connected peer. Returns the session ID.
#[tauri::command]
pub async fn send_file(
    state: State<'_, AppState>,
    peer_id: String,
    file_path: String,
) -> Result<String, CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    let session_id = engine
        .send_file(peer_id, PathBuf::from(file_path))
        .await
        .map_err(CommandError::from)?;

    Ok(session_id)
}

/// Accept an incoming transfer request.
#[tauri::command]
pub async fn accept_transfer(
    state: State<'_, AppState>,
    session_id: String,
    save_path: String,
) -> Result<(), CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    engine
        .accept_transfer(session_id, PathBuf::from(save_path))
        .await
        .map_err(CommandError::from)
}

/// Reject an incoming transfer request.
#[tauri::command]
pub async fn reject_transfer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    engine
        .reject_transfer(session_id)
        .await
        .map_err(CommandError::from)
}

/// Pause an active transfer.
#[tauri::command]
pub async fn pause_transfer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    engine
        .pause_transfer(session_id)
        .await
        .map_err(CommandError::from)
}

/// Cancel an active transfer.
#[tauri::command]
pub async fn cancel_transfer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    engine
        .cancel_transfer(session_id)
        .await
        .map_err(CommandError::from)
}

/// Resume a paused or interrupted transfer.
#[tauri::command]
pub async fn resume_transfer(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    engine
        .resume_transfer(session_id)
        .await
        .map_err(CommandError::from)
}

/// Get the transfer history — completed and failed sessions.
#[tauri::command]
pub async fn get_transfer_history(
    state: State<'_, AppState>,
) -> Result<Vec<TransferHistoryEntry>, CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    let sessions = engine.sessions();
    let mut history: Vec<TransferHistoryEntry> = sessions
        .iter()
        .filter_map(|entry| {
            let session: &P2PTransferSession = entry.value();
            let (status_str, failure_reason) = match &session.status {
                P2PTransferStatus::Completed => ("success".to_string(), None),
                P2PTransferStatus::Failed { reason } => {
                    ("failed".to_string(), Some(reason.clone()))
                }
                P2PTransferStatus::Cancelled => {
                    ("failed".to_string(), Some("cancelled by user".to_string()))
                }
                // Only include terminal states in history.
                _ => return None,
            };

            let direction = match session.direction {
                P2PTransferDirection::Sending => "sent",
                P2PTransferDirection::Receiving => "received",
            };

            Some(TransferHistoryEntry {
                session_id: session.id.clone(),
                file_name: session.file_name.clone(),
                direction: direction.to_string(),
                peer_display_name: session.remote_peer_name.clone(),
                timestamp: session.updated_at.to_rfc3339(),
                file_size: session.file_size,
                status: status_str,
                failure_reason,
            })
        })
        .collect();

    // Sort reverse chronologically.
    history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(history)
}

/// Get the current application settings.
#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<P2PSettings, CommandError> {
    let current = state.settings.read().await.clone();
    Ok(current)
}

/// Validate and save new application settings.
#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    new_settings: P2PSettings,
    app_handle: tauri::AppHandle,
) -> Result<(), CommandError> {
    // Validate first.
    let errors = settings::validate_p2p_settings(&new_settings);
    if !errors.is_empty() {
        return Err(CommandError::from(SettingsError::ValidationFailed {
            errors,
        }));
    }

    // Persist to disk.
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));

    settings::save_settings(&data_dir, &new_settings).map_err(CommandError::from)?;

    // Update in-memory settings.
    let mut settings_guard = state.settings.write().await;
    *settings_guard = new_settings;

    Ok(())
}

/// Get local peer info (display name, listen port, cert fingerprint).
#[tauri::command]
pub async fn get_local_info(state: State<'_, AppState>) -> Result<LocalInfo, CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;

    let info = engine.discovery().local_info();
    Ok(LocalInfo {
        display_name: info.display_name,
        listen_port: info.listen_port,
        cert_fingerprint: info.cert_fingerprint,
    })
}
