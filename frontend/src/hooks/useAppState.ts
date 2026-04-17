/**
 * Global application state that persists across tab switches.
 *
 * All Tauri event subscriptions live here so they are never torn down
 * when individual screen components unmount.
 */

import { createContext, useContext } from "react";
import type {
  PeerInfo,
  LocalInfo,
  TransferProgressPayload,
  IncomingTransferPayload,
  TransferHistoryEntry,
  P2PSettings,
} from "../services/p2pBridge";

// ── Transfer types ──────────────────────────────────────────────────────

export type SendStatus = "idle" | "sending" | "paused" | "completed" | "failed";

export interface SendState {
  selectedPeer: string;
  selectedFile: string | null;
  sessionId: string | null;
  status: SendStatus;
  progress: TransferProgressPayload | null;
  completedHash: string | null;
  error: string | null;
}

export interface IncomingRequest extends IncomingTransferPayload {
  status: "pending" | "accepted" | "rejected";
}

export interface ActiveReceive {
  sessionId: string;
  fileName: string;
  progress: TransferProgressPayload | null;
  status: "receiving" | "completed" | "failed";
  hash?: string;
  error?: string;
}

// ── Context shape ───────────────────────────────────────────────────────

export interface AppStateContextValue {
  // Peers
  localInfo: LocalInfo | null;
  peers: PeerInfo[];
  peersLoading: boolean;
  connectingId: string | null;
  connectError: string | null;
  handleConnect: (peerId: string, address: string) => Promise<void>;
  handleManualConnect: (address: string) => Promise<void>;

  // Send
  send: SendState;
  setSendSelectedPeer: (peerId: string) => void;
  setSendSelectedFile: (path: string | null) => void;
  handleSend: () => Promise<void>;
  handleSendPause: () => Promise<void>;
  handleSendCancel: () => Promise<void>;
  handleSendRetry: () => void;

  // Receive
  incoming: IncomingRequest[];
  activeReceives: ActiveReceive[];
  handleAccept: (req: IncomingRequest, savePath: string) => Promise<void>;
  handleReject: (sessionId: string) => Promise<void>;

  // History
  history: TransferHistoryEntry[];
  historyLoading: boolean;
  refreshHistory: () => void;

  // Settings
  settings: P2PSettings;
  settingsLoading: boolean;
  settingsSaved: boolean;
  settingsSaveError: string | null;
  setSettings: (s: P2PSettings) => void;
  handleSaveSettings: () => Promise<void>;
}

export const AppStateContext = createContext<AppStateContextValue | null>(null);

export function useAppState(): AppStateContextValue {
  const ctx = useContext(AppStateContext);
  if (!ctx) throw new Error("useAppState must be used within AppStateProvider");
  return ctx;
}
