/**
 * P2P Bridge — typed wrappers around Tauri IPC for the P2P engine.
 *
 * Replaces `apiClient.ts` (HTTP) and `connectionManager.ts` (QUIC protocol)
 * with direct Tauri `invoke()` commands and `listen()` event subscriptions.
 *
 * All types mirror the Rust structs defined in commands.rs, events.rs,
 * peer_registry.rs, and settings.rs.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ---------------------------------------------------------------------------
// Types — matching Rust structs
// ---------------------------------------------------------------------------

/** Peer connection status (mirrors `PeerStatus` enum in peer_registry.rs). */
export type PeerStatus = "Discovered" | "Connected" | "Unreachable";

/** Discovered peer info (mirrors `PeerInfo` in peer_registry.rs). */
export interface PeerInfo {
  id: string;
  display_name: string;
  addresses: string[];
  cert_fingerprint: string;
  status: PeerStatus;
  discovered_at: string;
  last_seen: string;
}

/** Returned after the P2P engine starts (mirrors `EngineInfo` in commands.rs). */
export interface EngineInfo {
  bound_port: number;
  fingerprint: string;
  display_name: string;
}

/** A single transfer history entry (mirrors `TransferHistoryEntry` in commands.rs). */
export interface TransferHistoryEntry {
  session_id: string;
  file_name: string;
  direction: string;
  peer_display_name: string;
  timestamp: string;
  file_size: number;
  status: string;
  failure_reason: string | null;
}

/** Local device info (mirrors `LocalInfo` in commands.rs). */
export interface LocalInfo {
  display_name: string;
  listen_port: number;
  cert_fingerprint: string;
}

/** Structured error from Tauri commands (mirrors `CommandError` in commands.rs). */
export interface CommandError {
  code: string;
  message: string;
}

/** User-facing settings (mirrors `P2PSettings` in settings.rs). */
export interface P2PSettings {
  display_name: string;
  listen_port: number;
  chunk_size: number;
  parallel_streams: number;
  per_transfer_rate_limit: number;
  download_dir: string;
}

// -- Event payloads (mirrors structs in events.rs) --

/** Payload for `peer-lost` event. */
export interface PeerLostPayload {
  peer_id: string;
}

/** Payload for `incoming-transfer-request` event. */
export interface IncomingTransferPayload {
  session_id: string;
  sender_name: string;
  file_name: string;
  file_size: number;
  whole_file_hash: string;
}

/** Payload for `transfer-progress` event. */
export interface TransferProgressPayload {
  session_id: string;
  file_name: string;
  direction: string;
  completed_chunks: number;
  total_chunks: number;
  percentage: number;
  speed_bps: number;
  eta_seconds: number;
}

/** Payload for `transfer-complete` event. */
export interface TransferCompletePayload {
  session_id: string;
  file_name: string;
  hash: string;
}

/** Payload for `transfer-failed` event. */
export interface TransferFailedPayload {
  session_id: string;
  reason: string;
}

// ---------------------------------------------------------------------------
// Tauri command wrappers (React → Rust)
// ---------------------------------------------------------------------------

/** Start the P2P engine. Returns engine info on success. */
export function startEngine(): Promise<EngineInfo> {
  return invoke<EngineInfo>("start_engine");
}

/** List all currently discovered peers. */
export function listPeers(): Promise<PeerInfo[]> {
  return invoke<PeerInfo[]>("list_peers");
}

/** Connect to a peer by address (e.g. "192.168.1.10:4433"). */
export function connectToPeer(address: string): Promise<PeerInfo> {
  return invoke<PeerInfo>("connect_to_peer", { address });
}

/** Send a file to a connected peer. Returns the session ID. */
export function sendFile(peerId: string, filePath: string): Promise<string> {
  return invoke<string>("send_file", { peerId, filePath });
}

/** Accept an incoming transfer request. */
export function acceptTransfer(
  sessionId: string,
  savePath: string,
): Promise<void> {
  return invoke<void>("accept_transfer", { sessionId, savePath });
}

/** Reject an incoming transfer request. */
export function rejectTransfer(sessionId: string): Promise<void> {
  return invoke<void>("reject_transfer", { sessionId });
}

/** Pause an active transfer. */
export function pauseTransfer(sessionId: string): Promise<void> {
  return invoke<void>("pause_transfer", { sessionId });
}

/** Cancel an active transfer. */
export function cancelTransfer(sessionId: string): Promise<void> {
  return invoke<void>("cancel_transfer", { sessionId });
}

/** Resume a paused or interrupted transfer. */
export function resumeTransfer(sessionId: string): Promise<void> {
  return invoke<void>("resume_transfer", { sessionId });
}

/** Get the transfer history (completed and failed sessions). */
export function getTransferHistory(): Promise<TransferHistoryEntry[]> {
  return invoke<TransferHistoryEntry[]>("get_transfer_history");
}

/** Get the current application settings. */
export function getSettings(): Promise<P2PSettings> {
  return invoke<P2PSettings>("get_settings");
}

/** Validate and save new application settings. */
export function saveSettings(newSettings: P2PSettings): Promise<void> {
  return invoke<void>("save_settings", { newSettings });
}

/** Get local device info (display name, port, fingerprint). */
export function getLocalInfo(): Promise<LocalInfo> {
  return invoke<LocalInfo>("get_local_info");
}

// ---------------------------------------------------------------------------
// Tauri event listeners (Rust → React)
// ---------------------------------------------------------------------------

/** Subscribe to new peer discovery events. Returns an unlisten function. */
export function onPeerDiscovered(
  callback: (peer: PeerInfo) => void,
): Promise<UnlistenFn> {
  return listen<PeerInfo>("peer-discovered", (event) => {
    callback(event.payload);
  });
}

/** Subscribe to peer lost events. Returns an unlisten function. */
export function onPeerLost(
  callback: (payload: PeerLostPayload) => void,
): Promise<UnlistenFn> {
  return listen<PeerLostPayload>("peer-lost", (event) => {
    callback(event.payload);
  });
}

/** Subscribe to incoming transfer request events. Returns an unlisten function. */
export function onIncomingTransfer(
  callback: (payload: IncomingTransferPayload) => void,
): Promise<UnlistenFn> {
  return listen<IncomingTransferPayload>(
    "incoming-transfer-request",
    (event) => {
      callback(event.payload);
    },
  );
}

/** Subscribe to transfer progress events. Returns an unlisten function. */
export function onTransferProgress(
  callback: (payload: TransferProgressPayload) => void,
): Promise<UnlistenFn> {
  return listen<TransferProgressPayload>("transfer-progress", (event) => {
    callback(event.payload);
  });
}

/** Subscribe to transfer complete events. Returns an unlisten function. */
export function onTransferComplete(
  callback: (payload: TransferCompletePayload) => void,
): Promise<UnlistenFn> {
  return listen<TransferCompletePayload>("transfer-complete", (event) => {
    callback(event.payload);
  });
}

/** Subscribe to transfer failed events. Returns an unlisten function. */
export function onTransferFailed(
  callback: (payload: TransferFailedPayload) => void,
): Promise<UnlistenFn> {
  return listen<TransferFailedPayload>("transfer-failed", (event) => {
    callback(event.payload);
  });
}
