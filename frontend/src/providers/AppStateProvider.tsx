import {
  useState,
  useCallback,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import {
  getLocalInfo,
  listPeers,
  connectToPeer,
  sendFile,
  pauseTransfer,
  cancelTransfer,
  acceptTransfer,
  rejectTransfer,
  getTransferHistory,
  getSettings,
  saveSettings as saveSettingsApi,
  onPeerDiscovered,
  onPeerLost,
  onIncomingTransfer,
  onTransferProgress,
  onTransferComplete,
  onTransferFailed,
  type PeerInfo,
  type LocalInfo,
  type TransferHistoryEntry,
  type P2PSettings,
} from "../services/p2pBridge";
import {
  AppStateContext,
  type SendState,
  type IncomingRequest,
  type ActiveReceive,
} from "../hooks/useAppState";

const DEFAULT_SETTINGS: P2PSettings = {
  display_name: "",
  listen_port: 4433,
  chunk_size: 4194304,
  parallel_streams: 4,
  per_transfer_rate_limit: 0,
  download_dir: "",
};

const INITIAL_SEND: SendState = {
  selectedPeer: "",
  selectedFile: null,
  sessionId: null,
  status: "idle",
  progress: null,
  completedHash: null,
  error: null,
};

export function AppStateProvider({ children }: { children: ReactNode }) {
  // ── Peers ───────────────────────────────────────────────────────────
  const [localInfo, setLocalInfo] = useState<LocalInfo | null>(null);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [peersLoading, setPeersLoading] = useState(true);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [connectError, setConnectError] = useState<string | null>(null);

  // ── Send ────────────────────────────────────────────────────────────
  const [send, setSend] = useState<SendState>(INITIAL_SEND);

  // ── Receive ─────────────────────────────────────────────────────────
  const [incoming, setIncoming] = useState<IncomingRequest[]>([]);
  const [activeReceives, setActiveReceives] = useState<ActiveReceive[]>([]);

  // ── History ─────────────────────────────────────────────────────────
  const [history, setHistory] = useState<TransferHistoryEntry[]>([]);
  const [historyLoading, setHistoryLoading] = useState(true);

  // ── Settings ────────────────────────────────────────────────────────
  const [settings, setSettings] = useState<P2PSettings>(DEFAULT_SETTINGS);
  const [settingsLoading, setSettingsLoading] = useState(true);
  const [settingsSaved, setSettingsSaved] = useState(false);
  const [settingsSaveError, setSettingsSaveError] = useState<string | null>(
    null,
  );

  // Refs for event handlers to access latest state without re-subscribing
  const sendRef = useRef(send);
  sendRef.current = send;

  // ── Initial data fetch (once) ───────────────────────────────────────
  useEffect(() => {
    Promise.all([getLocalInfo(), listPeers()])
      .then(([info, peerList]) => {
        setLocalInfo(info);
        setPeers(peerList);
      })
      .catch(() => {})
      .finally(() => setPeersLoading(false));

    getTransferHistory()
      .then(setHistory)
      .catch(() => {})
      .finally(() => setHistoryLoading(false));

    getSettings()
      .then(setSettings)
      .catch(() => {})
      .finally(() => setSettingsLoading(false));
  }, []);

  // ── Tauri event subscriptions (once, never torn down) ───────────────
  useEffect(() => {
    const unsubs: Promise<() => void>[] = [];

    unsubs.push(
      onPeerDiscovered((peer) => {
        setPeers((prev) => {
          const idx = prev.findIndex((p) => p.id === peer.id);
          if (idx >= 0) {
            const updated = [...prev];
            updated[idx] = peer;
            return updated;
          }
          return [...prev, peer];
        });
      }),
    );

    unsubs.push(
      onPeerLost((payload) => {
        setPeers((prev) => prev.filter((p) => p.id !== payload.peer_id));
      }),
    );

    unsubs.push(
      onIncomingTransfer((payload) => {
        setIncoming((prev) => [...prev, { ...payload, status: "pending" }]);
      }),
    );

    unsubs.push(
      onTransferProgress((p) => {
        // Update send progress
        const s = sendRef.current;
        if (s.sessionId && p.session_id === s.sessionId) {
          setSend((prev) => ({ ...prev, progress: p }));
        }
        // Update receive progress
        setActiveReceives((prev) =>
          prev.map((r) =>
            r.sessionId === p.session_id ? { ...r, progress: p } : r,
          ),
        );
      }),
    );

    unsubs.push(
      onTransferComplete((p) => {
        const s = sendRef.current;
        if (s.sessionId && p.session_id === s.sessionId) {
          setSend((prev) => ({
            ...prev,
            status: "completed",
            completedHash: p.hash,
          }));
        }
        setActiveReceives((prev) =>
          prev.map((r) =>
            r.sessionId === p.session_id
              ? { ...r, status: "completed", hash: p.hash }
              : r,
          ),
        );
        // Refresh history on completion
        getTransferHistory()
          .then(setHistory)
          .catch(() => {});
      }),
    );

    unsubs.push(
      onTransferFailed((p) => {
        const s = sendRef.current;
        if (s.sessionId && p.session_id === s.sessionId) {
          setSend((prev) => ({
            ...prev,
            status: "failed",
            error: p.reason,
          }));
        }
        setActiveReceives((prev) =>
          prev.map((r) =>
            r.sessionId === p.session_id
              ? { ...r, status: "failed", error: p.reason }
              : r,
          ),
        );
        getTransferHistory()
          .then(setHistory)
          .catch(() => {});
      }),
    );

    return () => {
      unsubs.forEach((p) => p.then((fn) => fn()));
    };
  }, []);

  // ── Peer actions ────────────────────────────────────────────────────
  const handleConnect = useCallback(async (peerId: string, address: string) => {
    setConnectingId(peerId);
    setConnectError(null);
    try {
      const updated = await connectToPeer(address);
      setPeers((prev) => prev.map((p) => (p.id === updated.id ? updated : p)));
    } catch (err) {
      const message = err instanceof Error ? err.message : "Connection failed";
      setConnectError(message);
    } finally {
      setConnectingId(null);
    }
  }, []);

  const handleManualConnect = useCallback(async (address: string) => {
    const addr = address.trim();
    if (!addr) return;
    setConnectingId("manual");
    setConnectError(null);
    try {
      const peer = await connectToPeer(addr);
      setPeers((prev) => {
        const idx = prev.findIndex((p) => p.id === peer.id);
        if (idx >= 0) {
          const updated = [...prev];
          updated[idx] = peer;
          return updated;
        }
        return [...prev, peer];
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Connection failed";
      setConnectError(message);
    } finally {
      setConnectingId(null);
    }
  }, []);

  // ── Send actions ────────────────────────────────────────────────────
  const setSendSelectedPeer = useCallback((peerId: string) => {
    setSend((prev) => ({ ...prev, selectedPeer: peerId }));
  }, []);

  const setSendSelectedFile = useCallback((path: string | null) => {
    setSend((prev) => ({ ...prev, selectedFile: path }));
  }, []);

  const handleSend = useCallback(async () => {
    const s = sendRef.current;
    if (!s.selectedFile || !s.selectedPeer) return;
    setSend((prev) => ({
      ...prev,
      status: "sending",
      error: null,
      progress: null,
      completedHash: null,
    }));
    try {
      const sid = await sendFile(s.selectedPeer, s.selectedFile);
      setSend((prev) => ({ ...prev, sessionId: sid }));
    } catch (err) {
      const message = err instanceof Error ? err.message : "Send failed";
      setSend((prev) => ({ ...prev, status: "failed", error: message }));
    }
  }, []);

  const handleSendPause = useCallback(async () => {
    const s = sendRef.current;
    if (!s.sessionId) return;
    try {
      await pauseTransfer(s.sessionId);
      setSend((prev) => ({ ...prev, status: "paused" }));
    } catch {}
  }, []);

  const handleSendCancel = useCallback(async () => {
    const s = sendRef.current;
    if (!s.sessionId) return;
    try {
      await cancelTransfer(s.sessionId);
    } catch {}
    setSend(INITIAL_SEND);
  }, []);

  const handleSendRetry = useCallback(() => {
    setSend((prev) => ({
      ...prev,
      sessionId: null,
      status: "idle",
      progress: null,
      error: null,
      completedHash: null,
    }));
  }, []);

  // ── Receive actions ─────────────────────────────────────────────────
  const handleAccept = useCallback(
    async (req: IncomingRequest, savePath: string) => {
      setIncoming((prev) =>
        prev.map((r) =>
          r.session_id === req.session_id ? { ...r, status: "accepted" } : r,
        ),
      );
      setActiveReceives((prev) => [
        ...prev,
        {
          sessionId: req.session_id,
          fileName: req.file_name,
          progress: null,
          status: "receiving",
        },
      ]);
      try {
        await acceptTransfer(req.session_id, savePath);
      } catch (err) {
        const message = err instanceof Error ? err.message : "Accept failed";
        setActiveReceives((prev) =>
          prev.map((r) =>
            r.sessionId === req.session_id
              ? { ...r, status: "failed", error: message }
              : r,
          ),
        );
      }
    },
    [],
  );

  const handleReject = useCallback(async (sessionId: string) => {
    setIncoming((prev) =>
      prev.map((r) =>
        r.session_id === sessionId ? { ...r, status: "rejected" } : r,
      ),
    );
    try {
      await rejectTransfer(sessionId);
    } catch {}
  }, []);

  // ── History actions ─────────────────────────────────────────────────
  const refreshHistory = useCallback(() => {
    getTransferHistory()
      .then(setHistory)
      .catch(() => {});
  }, []);

  // ── Settings actions ────────────────────────────────────────────────
  const handleSaveSettings = useCallback(async () => {
    setSettingsSaved(false);
    setSettingsSaveError(null);
    try {
      await saveSettingsApi(settings);
      setSettingsSaved(true);
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to save settings";
      setSettingsSaveError(message);
    }
  }, [settings]);

  return (
    <AppStateContext.Provider
      value={{
        localInfo,
        peers,
        peersLoading,
        connectingId,
        connectError,
        handleConnect,
        handleManualConnect,
        send,
        setSendSelectedPeer,
        setSendSelectedFile,
        handleSend,
        handleSendPause,
        handleSendCancel,
        handleSendRetry,
        incoming,
        activeReceives,
        handleAccept,
        handleReject,
        history,
        historyLoading,
        refreshHistory,
        settings,
        settingsLoading,
        settingsSaved,
        settingsSaveError,
        setSettings,
        handleSaveSettings,
      }}
    >
      {children}
    </AppStateContext.Provider>
  );
}
