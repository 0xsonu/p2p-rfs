use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{broadcast, oneshot, RwLock};
use tracing::{info, warn};

use crate::cert_manager::{CertError, CertManager};
use crate::discovery::{DiscoveryError, DiscoveryService};
use crate::peer_registry::{PeerId, PeerRegistry, PeerStatus};
use crate::settings::P2PConfig;

use integrity::{HashAlgorithm, IntegrityVerifier};
use protocol::{
    ChunkAck, ChunkData, PeerInfoExchange, Payload, ProtocolCodec, TransferAccept,
    TransferReject, TransferRequest,
};
use storage::{StorageEngine, StorageEngineConfig};
use transfer::session::{compute_chunk_layout, chunk_offset, SessionId};

/// Top-level errors produced by the P2P engine.
#[derive(Debug, Error)]
pub enum P2PError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("certificate error: {0}")]
    Cert(#[from] CertError),
    #[error("discovery error: {0}")]
    Discovery(#[from] DiscoveryError),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("peer not found: {0}")]
    PeerNotFound(String),
    #[error("engine not running")]
    NotRunning,
    #[error("engine already running")]
    AlreadyRunning,
    #[error("bind failed after exhausting port range")]
    BindFailed,
    #[error("transfer error: {0}")]
    Transfer(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("transfer rejected: {0}")]
    TransferRejected(String),
    #[error("integrity error: {0}")]
    Integrity(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("source file changed")]
    SourceFileChanged,
}

/// Maximum number of ports to try when the configured port is unavailable.
const PORT_FALLBACK_ATTEMPTS: u16 = 100;

/// Status of a P2P transfer session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum P2PTransferStatus {
    /// Waiting for the receiver to accept or reject.
    PendingAccept,
    /// Transfer is actively sending/receiving chunks.
    InProgress,
    /// Transfer is paused by the user.
    Paused,
    /// Transfer completed successfully.
    Completed,
    /// Transfer was cancelled by the user.
    Cancelled,
    /// Transfer failed with a reason.
    Failed { reason: String },
}

/// Direction of a P2P transfer from this peer's perspective.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum P2PTransferDirection {
    Sending,
    Receiving,
}

/// A P2P transfer session tracking chunk progress, hashes, and metadata.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct P2PTransferSession {
    pub id: SessionId,
    pub file_name: String,
    pub file_size: u64,
    pub whole_file_hash: String,
    pub hash_algorithm: String,
    pub chunk_size: usize,
    pub total_chunks: u64,
    pub completed_chunks: BTreeSet<u64>,
    pub chunk_hashes: HashMap<u64, String>,
    pub status: P2PTransferStatus,
    pub direction: P2PTransferDirection,
    pub remote_peer_id: PeerId,
    pub remote_peer_name: String,
    pub save_path: Option<PathBuf>,
    pub source_path: Option<PathBuf>,
    pub retry_counts: HashMap<u64, u32>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

impl P2PTransferSession {
    /// Check if all chunks are completed.
    pub fn is_all_chunks_complete(&self) -> bool {
        self.total_chunks > 0 && self.completed_chunks.len() as u64 == self.total_chunks
    }

    /// Find the first incomplete chunk index for resume.
    pub fn first_incomplete_chunk(&self) -> Option<u64> {
        (0..self.total_chunks).find(|i| !self.completed_chunks.contains(i))
    }
}

/// The P2P engine orchestrates QUIC listening, peer connections, and transfers.
pub struct P2PEngine {
    config: P2PConfig,
    cert_manager: Arc<CertManager>,
    discovery: Arc<DiscoveryService>,
    peer_registry: Arc<PeerRegistry>,
    codec: Arc<ProtocolCodec>,
    quic_endpoint: Arc<RwLock<Option<quinn::Endpoint>>>,
    active_connections: Arc<DashMap<PeerId, quinn::Connection>>,
    shutdown_tx: broadcast::Sender<()>,
    /// The actual port the QUIC listener bound to (may differ from config after fallback).
    bound_port: u16,
    /// Active transfer sessions (both sending and receiving).
    sessions: Arc<DashMap<SessionId, P2PTransferSession>>,
    /// Pending incoming transfers awaiting user accept/reject.
    pending_incoming: Arc<DashMap<SessionId, oneshot::Sender<Option<PathBuf>>>>,
    /// Integrity verifier for chunk/file hash operations.
    integrity: Arc<IntegrityVerifier>,
    /// Storage engine for writing received chunks to disk.
    storage_engine: Arc<StorageEngine>,
    /// Optional Tauri app handle for emitting events.
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
}

impl P2PEngine {
    /// Initialize all subsystems: generate/load certs, bind QUIC listener
    /// (with port fallback per Req 2.5), start mDNS discovery, and spawn
    /// the connection accept loop and idle-connection cleanup task.
    pub async fn start(config: P2PConfig) -> Result<Self, P2PError> {
        // 1. Load or generate TLS certificates.
        let cert_manager = Arc::new(CertManager::load_or_generate(&config.data_dir)?);
        info!(
            fingerprint = cert_manager.fingerprint(),
            "Certificate loaded"
        );

        // 2. Build QUIC server config from the certificate.
        let server_tls = cert_manager.server_tls_config()?;
        let server_config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_tls)
                .map_err(|e| P2PError::Transport(format!("QUIC server config: {e}")))?,
        ));

        // 3. Bind QUIC endpoint with port fallback (Req 2.5).
        let (endpoint, bound_port) =
            Self::bind_with_fallback(server_config, config.listen_port).await?;
        info!(port = bound_port, "QUIC listener bound");

        let endpoint = Arc::new(RwLock::new(Some(endpoint)));

        // 4. Create shared state.
        let peer_registry = Arc::new(PeerRegistry::new(Duration::from_secs(30)));
        let codec = Arc::new(ProtocolCodec::new(1, 1..=1));
        let active_connections: Arc<DashMap<PeerId, quinn::Connection>> =
            Arc::new(DashMap::new());
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let sessions: Arc<DashMap<SessionId, P2PTransferSession>> = Arc::new(DashMap::new());
        let pending_incoming: Arc<DashMap<SessionId, oneshot::Sender<Option<PathBuf>>>> =
            Arc::new(DashMap::new());

        // 5. Initialize integrity verifier and storage engine.
        let integrity = Arc::new(IntegrityVerifier::new(HashAlgorithm::Sha256));
        let storage_config = StorageEngineConfig {
            data_dir: config.download_dir.clone(),
            max_concurrent_writes: config.parallel_streams,
            write_buffer_size: config.chunk_size,
        };
        let storage_engine = Arc::new(StorageEngine::new(storage_config));

        // 6. Start mDNS discovery.
        let discovery = Arc::new(DiscoveryService::start(
            Arc::clone(&peer_registry),
            &config.display_name,
            bound_port,
            cert_manager.fingerprint(),
        )?);
        info!("mDNS discovery started");

        // 7. Spawn connection accept loop.
        {
            let ep = Arc::clone(&endpoint);
            let conns = Arc::clone(&active_connections);
            let registry = Arc::clone(&peer_registry);
            let codec_clone = Arc::clone(&codec);
            let cert_fp = cert_manager.fingerprint().to_string();
            let display = config.display_name.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            let sessions_clone = Arc::clone(&sessions);
            let pending_clone = Arc::clone(&pending_incoming);
            let integrity_clone = Arc::clone(&integrity);
            let storage_clone = Arc::clone(&storage_engine);
            let config_clone = config.clone();

            tokio::spawn(async move {
                Self::accept_loop(
                    ep,
                    conns,
                    registry,
                    codec_clone,
                    cert_fp,
                    display,
                    &mut shutdown_rx,
                    sessions_clone,
                    pending_clone,
                    integrity_clone,
                    storage_clone,
                    config_clone,
                )
                .await;
            });
        }

        // 8. Spawn idle connection cleanup task.
        {
            let conns = Arc::clone(&active_connections);
            let registry = Arc::clone(&peer_registry);
            let timeout = config.idle_connection_timeout;
            let mut shutdown_rx = shutdown_tx.subscribe();

            tokio::spawn(async move {
                Self::idle_cleanup_loop(conns, registry, timeout, &mut shutdown_rx).await;
            });
        }

        Ok(Self {
            config,
            cert_manager,
            discovery,
            peer_registry,
            codec,
            quic_endpoint: endpoint,
            active_connections,
            shutdown_tx,
            bound_port,
            sessions,
            pending_incoming,
            integrity,
            storage_engine,
            app_handle: Arc::new(RwLock::new(None)),
        })
    }

    /// Set the Tauri app handle for event emission.
    pub async fn set_app_handle(&self, handle: tauri::AppHandle) {
        let mut guard = self.app_handle.write().await;
        *guard = Some(handle);
    }

    /// Gracefully shut down: stop mDNS, close all QUIC connections, stop listener.
    pub async fn shutdown(&self) -> Result<(), P2PError> {
        info!("P2P engine shutting down");

        // Signal all background tasks to stop.
        let _ = self.shutdown_tx.send(());

        // Close all active QUIC connections.
        for entry in self.active_connections.iter() {
            entry.value().close(0u32.into(), b"shutdown");
        }
        self.active_connections.clear();

        // Stop mDNS discovery.
        self.discovery.stop()?;

        // Close the QUIC endpoint.
        let mut ep_guard = self.quic_endpoint.write().await;
        if let Some(ep) = ep_guard.take() {
            ep.close(0u32.into(), b"shutdown");
        }

        info!("P2P engine shut down");
        Ok(())
    }

    /// Connect to a specific peer by address. Establishes a QUIC connection,
    /// exchanges `PeerInfoExchange` messages, and stores the connection.
    pub async fn connect_to_peer(&self, addr: SocketAddr) -> Result<PeerId, P2PError> {
        let ep_guard = self.quic_endpoint.read().await;
        let ep = ep_guard.as_ref().ok_or(P2PError::NotRunning)?;

        // Build client TLS config (TOFU — accepts any self-signed cert).
        let client_tls = self.cert_manager.client_tls_config()?;
        let client_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_tls)
                .map_err(|e| P2PError::Transport(format!("QUIC client config: {e}")))?,
        ));

        let connection = ep
            .connect_with(client_config, addr, "fileshare")
            .map_err(|e| P2PError::Transport(format!("connect: {e}")))?
            .await
            .map_err(|e| P2PError::Transport(format!("connection failed: {e}")))?;

        // Exchange PeerInfoExchange on the first bidirectional stream.
        let peer_id = self.exchange_peer_info(&connection).await?;

        // Update peer registry status.
        self.peer_registry
            .set_status(&peer_id, PeerStatus::Connected);

        // Store the connection.
        self.active_connections
            .insert(peer_id.clone(), connection);

        info!(peer_id = %peer_id, addr = %addr, "Connected to peer");
        Ok(peer_id)
    }

    // -- File Send/Receive Orchestration ------------------------------------

    /// Initiate a file send to a connected peer.
    ///
    /// Computes the whole-file hash, creates a TransferSession, sends a
    /// `TransferRequest` over QUIC, waits for accept/reject, then sends
    /// chunks in parallel streams.
    pub async fn send_file(
        &self,
        peer_id: PeerId,
        file_path: PathBuf,
    ) -> Result<SessionId, P2PError> {
        // 1. Verify peer is connected.
        let connection = self
            .active_connections
            .get(&peer_id)
            .ok_or_else(|| P2PError::PeerNotFound(peer_id.clone()))?
            .clone();

        // 2. Read file metadata.
        let file_data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| P2PError::Io(format!("read file: {e}")))?;
        let file_size = file_data.len() as u64;
        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        // 3. Compute whole-file hash.
        let whole_hash = self
            .integrity
            .hash_chunk(&file_data, HashAlgorithm::Sha256);

        // 4. Compute chunk layout.
        let (total_chunks, _last_chunk_size) =
            compute_chunk_layout(file_size, self.config.chunk_size);

        // 5. Create transfer session.
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let peer_name = self
            .peer_registry
            .get(&peer_id)
            .map(|p| p.display_name.clone())
            .unwrap_or_else(|| peer_id.clone());

        let session = P2PTransferSession {
            id: session_id.clone(),
            file_name: file_name.clone(),
            file_size,
            whole_file_hash: whole_hash.value.clone(),
            hash_algorithm: whole_hash.algorithm.to_string(),
            chunk_size: self.config.chunk_size,
            total_chunks,
            completed_chunks: BTreeSet::new(),
            chunk_hashes: HashMap::new(),
            status: P2PTransferStatus::PendingAccept,
            direction: P2PTransferDirection::Sending,
            remote_peer_id: peer_id.clone(),
            remote_peer_name: peer_name,
            save_path: None,
            source_path: Some(file_path.clone()),
            retry_counts: HashMap::new(),
            created_at: now,
            updated_at: now,
        };
        self.sessions.insert(session_id.clone(), session);

        // 6. Send TransferRequest over QUIC.
        let transfer_request = TransferRequest {
            session_id: session_id.clone(),
            file_name,
            file_size,
            whole_file_hash: whole_hash.value.clone(),
            hash_algorithm: whole_hash.algorithm.to_string(),
            chunk_size: self.config.chunk_size as u64,
            total_chunks,
            sender_display_name: self.config.display_name.clone(),
        };

        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| P2PError::Transport(format!("open bi: {e}")))?;

        Self::send_message(
            &self.codec,
            &mut send,
            &Payload::TransferRequest(transfer_request),
        )
        .await?;

        // 7. Wait for accept/reject response.
        let response = Self::recv_message(&self.codec, &mut recv).await?;
        match response {
            Payload::TransferAccept(accept) if accept.session_id == session_id => {
                // Update session status to InProgress.
                if let Some(mut entry) = self.sessions.get_mut(&session_id) {
                    entry.status = P2PTransferStatus::InProgress;
                    entry.updated_at = Utc::now();
                }
            }
            Payload::TransferReject(reject) => {
                if let Some(mut entry) = self.sessions.get_mut(&session_id) {
                    entry.status = P2PTransferStatus::Failed {
                        reason: reject.reason.clone(),
                    };
                    entry.updated_at = Utc::now();
                }
                return Err(P2PError::TransferRejected(reject.reason));
            }
            _ => {
                return Err(P2PError::Protocol(
                    "expected TransferAccept or TransferReject".into(),
                ));
            }
        }

        // 8. Send chunks. Use the same bi-directional stream for simplicity.
        //    In a production system, we'd open parallel streams.
        let chunk_size = self.config.chunk_size;
        for chunk_index in 0..total_chunks {
            // Check if transfer was paused or cancelled.
            if let Some(entry) = self.sessions.get(&session_id) {
                match &entry.status {
                    P2PTransferStatus::Paused | P2PTransferStatus::Cancelled => {
                        return Ok(session_id);
                    }
                    _ => {}
                }
            }

            let offset = chunk_offset(chunk_index, chunk_size) as usize;
            let end = std::cmp::min(offset + chunk_size, file_data.len());
            let chunk_data_slice = &file_data[offset..end];

            let chunk_hash = self
                .integrity
                .hash_chunk(chunk_data_slice, HashAlgorithm::Sha256);

            let chunk_msg = ChunkData {
                session_id: session_id.clone(),
                chunk_index,
                offset: offset as u64,
                data: chunk_data_slice.to_vec(),
                hash: chunk_hash.value.clone(),
                hash_algorithm: chunk_hash.algorithm.to_string(),
            };

            Self::send_message(&self.codec, &mut send, &Payload::ChunkData(chunk_msg)).await?;

            // Wait for ChunkAck.
            let ack = Self::recv_message(&self.codec, &mut recv).await?;
            match ack {
                Payload::ChunkAck(ack) if ack.chunk_index == chunk_index => {
                    if let Some(mut entry) = self.sessions.get_mut(&session_id) {
                        entry.completed_chunks.insert(chunk_index);
                        entry
                            .chunk_hashes
                            .insert(chunk_index, chunk_hash.value.clone());
                        entry.updated_at = Utc::now();
                    }
                }
                _ => {
                    return Err(P2PError::Protocol(format!(
                        "expected ChunkAck for index {chunk_index}"
                    )));
                }
            }
        }

        // 9. Mark session as complete.
        if let Some(mut entry) = self.sessions.get_mut(&session_id) {
            entry.status = P2PTransferStatus::Completed;
            entry.updated_at = Utc::now();
        }

        info!(session_id = %session_id, "File send completed");
        Ok(session_id)
    }

    /// Accept an incoming transfer request. Sends `TransferAccept` and begins
    /// receiving chunks.
    pub async fn accept_transfer(
        &self,
        session_id: SessionId,
        save_path: PathBuf,
    ) -> Result<(), P2PError> {
        // Send the accept decision to the waiting handler.
        if let Some((_, tx)) = self.pending_incoming.remove(&session_id) {
            tx.send(Some(save_path.clone()))
                .map_err(|_| P2PError::Transfer("accept channel closed".into()))?;
        } else {
            return Err(P2PError::SessionNotFound(session_id));
        }

        // Update session with save path.
        if let Some(mut entry) = self.sessions.get_mut(&session_id) {
            entry.save_path = Some(save_path);
            entry.updated_at = Utc::now();
        }

        Ok(())
    }

    /// Reject an incoming transfer request. Sends `TransferReject`.
    pub async fn reject_transfer(&self, session_id: SessionId) -> Result<(), P2PError> {
        if let Some((_, tx)) = self.pending_incoming.remove(&session_id) {
            tx.send(None)
                .map_err(|_| P2PError::Transfer("reject channel closed".into()))?;
        } else {
            return Err(P2PError::SessionNotFound(session_id));
        }

        // Update session status.
        if let Some(mut entry) = self.sessions.get_mut(&session_id) {
            entry.status = P2PTransferStatus::Failed {
                reason: "rejected by user".into(),
            };
            entry.updated_at = Utc::now();
        }

        Ok(())
    }

    /// Pause an active transfer.
    pub async fn pause_transfer(&self, session_id: SessionId) -> Result<(), P2PError> {
        if let Some(mut entry) = self.sessions.get_mut(&session_id) {
            if entry.status == P2PTransferStatus::InProgress {
                entry.status = P2PTransferStatus::Paused;
                entry.updated_at = Utc::now();
                info!(session_id = %session_id, "Transfer paused");
                Ok(())
            } else {
                Err(P2PError::Transfer(format!(
                    "cannot pause transfer in state {:?}",
                    entry.status
                )))
            }
        } else {
            Err(P2PError::SessionNotFound(session_id))
        }
    }

    /// Cancel an active transfer.
    pub async fn cancel_transfer(&self, session_id: SessionId) -> Result<(), P2PError> {
        if let Some(mut entry) = self.sessions.get_mut(&session_id) {
            match &entry.status {
                P2PTransferStatus::InProgress
                | P2PTransferStatus::Paused
                | P2PTransferStatus::PendingAccept => {
                    entry.status = P2PTransferStatus::Cancelled;
                    entry.updated_at = Utc::now();
                    info!(session_id = %session_id, "Transfer cancelled");
                    Ok(())
                }
                _ => Err(P2PError::Transfer(format!(
                    "cannot cancel transfer in state {:?}",
                    entry.status
                ))),
            }
        } else {
            Err(P2PError::SessionNotFound(session_id))
        }
    }

    /// Resume a paused or interrupted transfer.
    pub async fn resume_transfer(&self, session_id: SessionId) -> Result<(), P2PError> {
        if let Some(mut entry) = self.sessions.get_mut(&session_id) {
            if entry.status == P2PTransferStatus::Paused {
                entry.status = P2PTransferStatus::InProgress;
                entry.updated_at = Utc::now();
                info!(session_id = %session_id, "Transfer resumed");
                Ok(())
            } else {
                Err(P2PError::Transfer(format!(
                    "cannot resume transfer in state {:?}",
                    entry.status
                )))
            }
        } else {
            Err(P2PError::SessionNotFound(session_id))
        }
    }

    // -- Public accessors --------------------------------------------------

    /// The actual port the QUIC listener is bound to.
    pub fn bound_port(&self) -> u16 {
        self.bound_port
    }

    /// Reference to the shared peer registry.
    pub fn peer_registry(&self) -> &Arc<PeerRegistry> {
        &self.peer_registry
    }

    /// Reference to the discovery service.
    pub fn discovery(&self) -> &Arc<DiscoveryService> {
        &self.discovery
    }

    /// Reference to the certificate manager.
    pub fn cert_manager(&self) -> &Arc<CertManager> {
        &self.cert_manager
    }

    /// Reference to the protocol codec.
    pub fn codec(&self) -> &Arc<ProtocolCodec> {
        &self.codec
    }

    /// Reference to the active connections map.
    pub fn active_connections(&self) -> &Arc<DashMap<PeerId, quinn::Connection>> {
        &self.active_connections
    }

    /// Reference to the engine config.
    pub fn config(&self) -> &P2PConfig {
        &self.config
    }

    /// Reference to the transfer sessions map.
    pub fn sessions(&self) -> &Arc<DashMap<SessionId, P2PTransferSession>> {
        &self.sessions
    }

    /// Get a clone of a specific session.
    pub fn get_session(&self, session_id: &str) -> Option<P2PTransferSession> {
        self.sessions.get(session_id).map(|e| e.clone())
    }

    // -- Private helpers ---------------------------------------------------

    /// Try binding a QUIC endpoint on `port`, falling back to successive ports
    /// up to `PORT_FALLBACK_ATTEMPTS` times (Req 2.5).
    async fn bind_with_fallback(
        server_config: quinn::ServerConfig,
        start_port: u16,
    ) -> Result<(quinn::Endpoint, u16), P2PError> {
        for offset in 0..PORT_FALLBACK_ATTEMPTS {
            let port = start_port.wrapping_add(offset);
            if port == 0 {
                continue; // skip port 0
            }
            let addr: SocketAddr = ([0, 0, 0, 0], port).into();
            match quinn::Endpoint::server(server_config.clone(), addr) {
                Ok(ep) => return Ok((ep, port)),
                Err(e) => {
                    warn!(port = port, error = %e, "Port bind failed, trying next");
                }
            }
        }
        Err(P2PError::BindFailed)
    }

    /// Send a length-prefixed protocol message over a QUIC send stream.
    async fn send_message(
        codec: &ProtocolCodec,
        send: &mut quinn::SendStream,
        payload: &Payload,
    ) -> Result<(), P2PError> {
        let correlation_id = uuid::Uuid::new_v4().to_string();
        let encoded = codec
            .encode(payload, &correlation_id)
            .map_err(|e| P2PError::Protocol(e.to_string()))?;

        let len = (encoded.len() as u32).to_be_bytes();
        send.write_all(&len)
            .await
            .map_err(|e| P2PError::Transport(format!("write len: {e}")))?;
        send.write_all(&encoded)
            .await
            .map_err(|e| P2PError::Transport(format!("write payload: {e}")))?;
        Ok(())
    }

    /// Receive a length-prefixed protocol message from a QUIC recv stream.
    async fn recv_message(
        codec: &ProtocolCodec,
        recv: &mut quinn::RecvStream,
    ) -> Result<Payload, P2PError> {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| P2PError::Transport(format!("read len: {e}")))?;
        let msg_len = u32::from_be_bytes(len_buf) as usize;

        if msg_len > 64 * 1024 * 1024 {
            return Err(P2PError::Protocol("message too large".into()));
        }

        let mut buf = vec![0u8; msg_len];
        recv.read_exact(&mut buf)
            .await
            .map_err(|e| P2PError::Transport(format!("read payload: {e}")))?;

        let (payload, _correlation_id) = codec
            .decode(&buf)
            .map_err(|e| P2PError::Protocol(e.to_string()))?;

        Ok(payload)
    }

    /// Exchange `PeerInfoExchange` messages with a remote peer over the first
    /// bidirectional QUIC stream. Returns the remote peer's ID (cert fingerprint).
    async fn exchange_peer_info(
        &self,
        connection: &quinn::Connection,
    ) -> Result<PeerId, P2PError> {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| P2PError::Transport(format!("open bi stream: {e}")))?;

        // Build our PeerInfoExchange message.
        let local_info = PeerInfoExchange {
            display_name: self.config.display_name.clone(),
            cert_fingerprint: self.cert_manager.fingerprint().to_string(),
            protocol_version: self.codec.current_version(),
        };

        Self::send_message(&self.codec, &mut send, &Payload::PeerInfoExchange(local_info))
            .await?;
        send.finish()
            .map_err(|e| P2PError::Transport(format!("finish send: {e}")))?;

        // Read remote peer's response.
        let remote_payload = Self::recv_message(&self.codec, &mut recv).await?;

        match remote_payload {
            Payload::PeerInfoExchange(info) => {
                let peer_id = if info.cert_fingerprint.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    info.cert_fingerprint.clone()
                };
                Ok(peer_id)
            }
            _ => Err(P2PError::Protocol(
                "expected PeerInfoExchange, got different message".into(),
            )),
        }
    }

    /// Handle an incoming `PeerInfoExchange` from a newly accepted connection.
    /// Returns the remote peer's ID.
    async fn handle_incoming_peer_info(
        codec: &ProtocolCodec,
        config_display_name: &str,
        cert_fingerprint: &str,
        connection: &quinn::Connection,
    ) -> Result<PeerId, P2PError> {
        let (mut send, mut recv) = connection
            .accept_bi()
            .await
            .map_err(|e| P2PError::Transport(format!("accept bi stream: {e}")))?;

        // Read remote peer's message.
        let remote_payload = Self::recv_message(codec, &mut recv).await?;

        let peer_id = match &remote_payload {
            Payload::PeerInfoExchange(info) => {
                if info.cert_fingerprint.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    info.cert_fingerprint.clone()
                }
            }
            _ => {
                return Err(P2PError::Protocol(
                    "expected PeerInfoExchange, got different message".into(),
                ));
            }
        };

        // Send our PeerInfoExchange back.
        let local_info = PeerInfoExchange {
            display_name: config_display_name.to_string(),
            cert_fingerprint: cert_fingerprint.to_string(),
            protocol_version: codec.current_version(),
        };
        Self::send_message(codec, &mut send, &Payload::PeerInfoExchange(local_info)).await?;
        send.finish()
            .map_err(|e| P2PError::Transport(format!("finish send: {e}")))?;

        Ok(peer_id)
    }

    /// Handle an incoming transfer request on a bidirectional stream.
    /// Creates a pending session, waits for user accept/reject, then
    /// receives chunks or sends reject.
    async fn handle_incoming_transfer(
        codec: Arc<ProtocolCodec>,
        sessions: Arc<DashMap<SessionId, P2PTransferSession>>,
        pending_incoming: Arc<DashMap<SessionId, oneshot::Sender<Option<PathBuf>>>>,
        integrity: Arc<IntegrityVerifier>,
        _storage: Arc<StorageEngine>,
        config: P2PConfig,
        request: TransferRequest,
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
        remote_peer_id: PeerId,
    ) {
        let session_id = request.session_id.clone();
        let now = Utc::now();

        // Create the pending session.
        let session = P2PTransferSession {
            id: session_id.clone(),
            file_name: request.file_name.clone(),
            file_size: request.file_size,
            whole_file_hash: request.whole_file_hash.clone(),
            hash_algorithm: request.hash_algorithm.clone(),
            chunk_size: request.chunk_size as usize,
            total_chunks: request.total_chunks,
            completed_chunks: BTreeSet::new(),
            chunk_hashes: HashMap::new(),
            status: P2PTransferStatus::PendingAccept,
            direction: P2PTransferDirection::Receiving,
            remote_peer_id: remote_peer_id.clone(),
            remote_peer_name: request.sender_display_name.clone(),
            save_path: None,
            source_path: None,
            retry_counts: HashMap::new(),
            created_at: now,
            updated_at: now,
        };
        sessions.insert(session_id.clone(), session);

        // Create a oneshot channel for the user's accept/reject decision.
        let (tx, rx) = oneshot::channel::<Option<PathBuf>>();
        pending_incoming.insert(session_id.clone(), tx);

        // TODO: Emit Tauri event `incoming-transfer-request` here when app_handle is available.
        info!(
            session_id = %session_id,
            file_name = %request.file_name,
            file_size = request.file_size,
            sender = %request.sender_display_name,
            "Incoming transfer request"
        );

        // Wait for user decision.
        let decision = match rx.await {
            Ok(decision) => decision,
            Err(_) => {
                // Channel dropped — treat as reject.
                warn!(session_id = %session_id, "Accept channel dropped, rejecting");
                None
            }
        };

        match decision {
            Some(save_path) => {
                // User accepted — send TransferAccept.
                let accept = TransferAccept {
                    session_id: session_id.clone(),
                };
                if let Err(e) =
                    Self::send_message(&codec, &mut send, &Payload::TransferAccept(accept)).await
                {
                    warn!(error = %e, "Failed to send TransferAccept");
                    return;
                }

                // Update session status.
                if let Some(mut entry) = sessions.get_mut(&session_id) {
                    entry.status = P2PTransferStatus::InProgress;
                    entry.save_path = Some(save_path.clone());
                    entry.updated_at = Utc::now();
                }

                // Pre-allocate the file on disk using the save_path file name as file_id.
                let file_id = session_id.clone();
                // Ensure download directory exists and allocate file.
                let download_dir = save_path.parent().unwrap_or(&config.download_dir);
                let file_storage = StorageEngine::new(StorageEngineConfig {
                    data_dir: download_dir.to_path_buf(),
                    max_concurrent_writes: config.parallel_streams,
                    write_buffer_size: config.chunk_size,
                });
                let actual_file_name = save_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or(file_id.clone());

                if let Err(e) = file_storage
                    .allocate_file(actual_file_name.clone(), request.file_size)
                    .await
                {
                    warn!(error = %e, "Failed to allocate file for incoming transfer");
                    if let Some(mut entry) = sessions.get_mut(&session_id) {
                        entry.status = P2PTransferStatus::Failed {
                            reason: format!("storage allocation failed: {e}"),
                        };
                    }
                    return;
                }

                // Receive chunks loop.
                let chunk_size = request.chunk_size as usize;
                let max_retries = config.max_retries;
                for _expected in 0..request.total_chunks {
                    // Check if cancelled/paused.
                    if let Some(entry) = sessions.get(&session_id) {
                        match &entry.status {
                            P2PTransferStatus::Cancelled | P2PTransferStatus::Paused => {
                                return;
                            }
                            _ => {}
                        }
                    }

                    let msg = match Self::recv_message(&codec, &mut recv).await {
                        Ok(msg) => msg,
                        Err(e) => {
                            warn!(error = %e, "Failed to receive chunk");
                            if let Some(mut entry) = sessions.get_mut(&session_id) {
                                entry.status = P2PTransferStatus::Failed {
                                    reason: format!("receive error: {e}"),
                                };
                            }
                            return;
                        }
                    };

                    match msg {
                        Payload::ChunkData(chunk) => {
                            // Verify chunk hash.
                            let expected_hash = integrity::ChunkHash {
                                algorithm: HashAlgorithm::Sha256,
                                value: chunk.hash.clone(),
                            };
                            if let Err(e) = integrity.verify_chunk(&chunk.data, &expected_hash) {
                                warn!(
                                    chunk_index = chunk.chunk_index,
                                    error = %e,
                                    "Chunk hash verification failed"
                                );
                                // Track retry.
                                if let Some(mut entry) = sessions.get_mut(&session_id) {
                                    let count = entry
                                        .retry_counts
                                        .entry(chunk.chunk_index)
                                        .or_insert(0);
                                    *count += 1;
                                    if *count > max_retries {
                                        entry.status = P2PTransferStatus::Failed {
                                            reason: format!(
                                                "chunk {} failed after {} retries",
                                                chunk.chunk_index, max_retries
                                            ),
                                        };
                                        return;
                                    }
                                }
                                continue;
                            }

                            // Write chunk to disk.
                            if let Err(e) = file_storage
                                .write_chunk(
                                    actual_file_name.clone(),
                                    chunk.offset,
                                    &chunk.data,
                                )
                                .await
                            {
                                warn!(error = %e, "Failed to write chunk to disk");
                                if let Some(mut entry) = sessions.get_mut(&session_id) {
                                    entry.status = P2PTransferStatus::Failed {
                                        reason: format!("write failed: {e}"),
                                    };
                                }
                                return;
                            }

                            // Send ChunkAck.
                            let ack = ChunkAck {
                                session_id: session_id.clone(),
                                chunk_index: chunk.chunk_index,
                            };
                            if let Err(e) =
                                Self::send_message(&codec, &mut send, &Payload::ChunkAck(ack))
                                    .await
                            {
                                warn!(error = %e, "Failed to send ChunkAck");
                                return;
                            }

                            // Update session state.
                            if let Some(mut entry) = sessions.get_mut(&session_id) {
                                entry.completed_chunks.insert(chunk.chunk_index);
                                entry
                                    .chunk_hashes
                                    .insert(chunk.chunk_index, chunk.hash.clone());
                                entry.updated_at = Utc::now();
                            }
                        }
                        _ => {
                            warn!("Unexpected message during chunk receive");
                        }
                    }
                }

                // Verify whole-file hash after all chunks received.
                let session_complete = sessions
                    .get(&session_id)
                    .map(|s| s.is_all_chunks_complete())
                    .unwrap_or(false);

                if session_complete {
                    // Read all chunks back and verify whole-file hash.
                    let mut all_data: Vec<Vec<u8>> = Vec::new();
                    let total = request.total_chunks;
                    for i in 0..total {
                        let offset = chunk_offset(i, chunk_size);
                        let len = if i == total - 1 {
                            let (_, last) = compute_chunk_layout(request.file_size, chunk_size);
                            last
                        } else {
                            chunk_size
                        };
                        match file_storage
                            .read_chunk(actual_file_name.clone(), offset, len)
                            .await
                        {
                            Ok(data) => all_data.push(data),
                            Err(e) => {
                                warn!(error = %e, "Failed to read chunk for verification");
                                if let Some(mut entry) = sessions.get_mut(&session_id) {
                                    entry.status = P2PTransferStatus::Failed {
                                        reason: format!("verification read failed: {e}"),
                                    };
                                }
                                return;
                            }
                        }
                    }

                    let slices: Vec<&[u8]> = all_data.iter().map(|d| d.as_slice()).collect();
                    if let Err(e) = integrity
                        .verify_file_from_chunks(slices.into_iter(), &request.whole_file_hash)
                    {
                        warn!(error = %e, "Whole-file hash verification failed");
                        if let Some(mut entry) = sessions.get_mut(&session_id) {
                            entry.status = P2PTransferStatus::Failed {
                                reason: format!("file hash mismatch: {e}"),
                            };
                        }
                        return;
                    }

                    // Mark complete.
                    if let Some(mut entry) = sessions.get_mut(&session_id) {
                        entry.status = P2PTransferStatus::Completed;
                        entry.updated_at = Utc::now();
                    }
                    info!(session_id = %session_id, "File receive completed");
                }
            }
            None => {
                // User rejected — send TransferReject.
                let reject = TransferReject {
                    session_id: session_id.clone(),
                    reason: "rejected by user".into(),
                };
                let _ =
                    Self::send_message(&codec, &mut send, &Payload::TransferReject(reject)).await;

                if let Some(mut entry) = sessions.get_mut(&session_id) {
                    entry.status = P2PTransferStatus::Failed {
                        reason: "rejected by user".into(),
                    };
                    entry.updated_at = Utc::now();
                }
            }
        }
    }

    /// Background loop that accepts incoming QUIC connections, exchanges
    /// `PeerInfoExchange`, and stores them in `active_connections`.
    /// Also listens for incoming transfer requests on subsequent streams.
    #[allow(clippy::too_many_arguments)]
    async fn accept_loop(
        endpoint: Arc<RwLock<Option<quinn::Endpoint>>>,
        active_connections: Arc<DashMap<PeerId, quinn::Connection>>,
        peer_registry: Arc<PeerRegistry>,
        codec: Arc<ProtocolCodec>,
        cert_fingerprint: String,
        display_name: String,
        shutdown_rx: &mut broadcast::Receiver<()>,
        sessions: Arc<DashMap<SessionId, P2PTransferSession>>,
        pending_incoming: Arc<DashMap<SessionId, oneshot::Sender<Option<PathBuf>>>>,
        integrity: Arc<IntegrityVerifier>,
        storage: Arc<StorageEngine>,
        config: P2PConfig,
    ) {
        loop {
            // Get a reference to the endpoint for this iteration.
            let incoming = {
                let ep_guard = endpoint.read().await;
                let ep = match ep_guard.as_ref() {
                    Some(ep) => ep.clone(),
                    None => {
                        info!("Endpoint closed, stopping accept loop");
                        return;
                    }
                };
                drop(ep_guard);

                tokio::select! {
                    incoming = ep.accept() => incoming,
                    _ = shutdown_rx.recv() => {
                        info!("Shutdown signal received, stopping accept loop");
                        return;
                    }
                }
            };

            let incoming = match incoming {
                Some(inc) => inc,
                None => {
                    info!("Endpoint closed, stopping accept loop");
                    return;
                }
            };

            let conns = Arc::clone(&active_connections);
            let registry = Arc::clone(&peer_registry);
            let codec_clone = Arc::clone(&codec);
            let fp = cert_fingerprint.clone();
            let name = display_name.clone();
            let sessions_clone = Arc::clone(&sessions);
            let pending_clone = Arc::clone(&pending_incoming);
            let integrity_clone = Arc::clone(&integrity);
            let storage_clone = Arc::clone(&storage);
            let config_clone = config.clone();

            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        let remote_addr = connection.remote_address();
                        match Self::handle_incoming_peer_info(
                            &codec_clone,
                            &name,
                            &fp,
                            &connection,
                        )
                        .await
                        {
                            Ok(peer_id) => {
                                registry.set_status(&peer_id, PeerStatus::Connected);
                                conns.insert(peer_id.clone(), connection.clone());
                                info!(
                                    peer_id = %peer_id,
                                    addr = %remote_addr,
                                    "Accepted incoming peer connection"
                                );

                                // Spawn a task to listen for incoming transfer
                                // requests on subsequent bi-directional streams.
                                let codec_for_transfer = Arc::clone(&codec_clone);
                                let sessions_for_transfer = Arc::clone(&sessions_clone);
                                let pending_for_transfer = Arc::clone(&pending_clone);
                                let integrity_for_transfer = Arc::clone(&integrity_clone);
                                let storage_for_transfer = Arc::clone(&storage_clone);
                                let config_for_transfer = config_clone.clone();
                                let peer_id_for_transfer = peer_id.clone();

                                tokio::spawn(async move {
                                    loop {
                                        let (send, mut recv) = match connection.accept_bi().await {
                                            Ok(streams) => streams,
                                            Err(_) => break, // connection closed
                                        };

                                        // Read the first message to determine type.
                                        let msg = match Self::recv_message(
                                            &codec_for_transfer,
                                            &mut recv,
                                        )
                                        .await
                                        {
                                            Ok(msg) => msg,
                                            Err(_) => break,
                                        };

                                        if let Payload::TransferRequest(request) = msg {
                                            let codec_c = Arc::clone(&codec_for_transfer);
                                            let sess_c = Arc::clone(&sessions_for_transfer);
                                            let pend_c = Arc::clone(&pending_for_transfer);
                                            let integ_c = Arc::clone(&integrity_for_transfer);
                                            let stor_c = Arc::clone(&storage_for_transfer);
                                            let conf_c = config_for_transfer.clone();
                                            let pid = peer_id_for_transfer.clone();

                                            tokio::spawn(async move {
                                                Self::handle_incoming_transfer(
                                                    codec_c, sess_c, pend_c, integ_c, stor_c,
                                                    conf_c, request, send, recv, pid,
                                                )
                                                .await;
                                            });
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                warn!(
                                    addr = %remote_addr,
                                    error = %e,
                                    "Failed peer info exchange on incoming connection"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to accept incoming QUIC connection");
                    }
                }
            });
        }
    }

    /// Background loop that periodically checks for idle connections and
    /// closes them after `idle_timeout` (Req 4.5).
    async fn idle_cleanup_loop(
        active_connections: Arc<DashMap<PeerId, quinn::Connection>>,
        peer_registry: Arc<PeerRegistry>,
        _idle_timeout: Duration,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal, stopping idle cleanup");
                    return;
                }
            }

            let mut to_remove = Vec::new();
            for entry in active_connections.iter() {
                let conn = entry.value();
                if conn.close_reason().is_some() {
                    to_remove.push(entry.key().clone());
                }
            }

            for peer_id in &to_remove {
                active_connections.remove(peer_id);
                peer_registry.set_status(peer_id, PeerStatus::Discovered);
                info!(peer_id = %peer_id, "Removed closed/idle connection");
            }
        }
    }
}
