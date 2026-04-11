import { useState, useCallback, useEffect } from "react";
import { openFileDialog } from "../services/tauriBridge";
import {
  listPeers,
  sendFile,
  pauseTransfer,
  cancelTransfer,
  onTransferProgress,
  onTransferComplete,
  onTransferFailed,
  type PeerInfo,
  type TransferProgressPayload,
} from "../services/p2pBridge";
import { TransferProgress } from "../components/TransferProgress";

type SendStatus = "idle" | "sending" | "paused" | "completed" | "failed";

/**
 * Send screen — pick a file via native dialog, choose a connected peer,
 * and send with real-time progress, pause/cancel controls.
 *
 * Requirements: 5.1, 12.1, 12.2, 12.3, 12.4, 15.1–15.5
 */
export function UploadScreen() {
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [selectedPeer, setSelectedPeer] = useState<string>("");
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [status, setStatus] = useState<SendStatus>("idle");
  const [progress, setProgress] = useState<TransferProgressPayload | null>(
    null,
  );
  const [completedHash, setCompletedHash] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Load connected peers
  useEffect(() => {
    listPeers()
      .then((list) => setPeers(list.filter((p) => p.status === "Connected")))
      .catch(() => {});
  }, []);

  // Subscribe to transfer events
  useEffect(() => {
    const unProgress = onTransferProgress((p) => {
      if (sessionId && p.session_id === sessionId) {
        setProgress(p);
      }
    });
    const unComplete = onTransferComplete((p) => {
      if (sessionId && p.session_id === sessionId) {
        setStatus("completed");
        setCompletedHash(p.hash);
      }
    });
    const unFailed = onTransferFailed((p) => {
      if (sessionId && p.session_id === sessionId) {
        setStatus("failed");
        setError(p.reason);
      }
    });

    return () => {
      unProgress.then((fn) => fn());
      unComplete.then((fn) => fn());
      unFailed.then((fn) => fn());
    };
  }, [sessionId]);

  const handlePickFile = useCallback(async () => {
    const paths = await openFileDialog({
      multiple: false,
      title: "Select file to send",
    });
    if (paths.length > 0) {
      setSelectedFile(paths[0]);
    }
  }, []);

  const handleSend = useCallback(async () => {
    if (!selectedFile || !selectedPeer) return;
    setStatus("sending");
    setError(null);
    setProgress(null);
    setCompletedHash(null);
    try {
      const sid = await sendFile(selectedPeer, selectedFile);
      setSessionId(sid);
    } catch (err) {
      const message = err instanceof Error ? err.message : "Send failed";
      setError(message);
      setStatus("failed");
    }
  }, [selectedFile, selectedPeer]);

  const handlePause = useCallback(async () => {
    if (!sessionId) return;
    try {
      await pauseTransfer(sessionId);
      setStatus("paused");
    } catch {}
  }, [sessionId]);

  const handleCancel = useCallback(async () => {
    if (!sessionId) return;
    try {
      await cancelTransfer(sessionId);
    } catch {}
    setStatus("idle");
    setSessionId(null);
    setProgress(null);
  }, [sessionId]);

  const handleRetry = useCallback(() => {
    setStatus("idle");
    setSessionId(null);
    setProgress(null);
    setError(null);
    setCompletedHash(null);
  }, []);

  const isActive = status === "sending" || status === "paused";

  // Build progress object for TransferProgress component
  const progressData = progress
    ? {
        percentage: progress.percentage,
        speed: progress.speed_bps,
        eta: progress.eta_seconds,
        completedChunks: progress.completed_chunks,
        totalChunks: progress.total_chunks,
      }
    : { percentage: 0, speed: 0, eta: 0, completedChunks: 0, totalChunks: 0 };

  return (
    <div className="space-y-6">
      <h2 className="text-sm font-semibold text-gray-700 uppercase tracking-wide">
        Send File
      </h2>

      {/* Peer selection + file picker */}
      <div className="bg-white rounded-lg shadow p-4 space-y-4">
        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Send to peer
          </label>
          <select
            value={selectedPeer}
            onChange={(e) => setSelectedPeer(e.target.value)}
            disabled={isActive}
            className="w-full rounded border border-gray-300 px-3 py-2 text-sm"
          >
            <option value="">Select a connected peer…</option>
            {peers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.display_name} ({p.addresses[0] ?? "unknown"})
              </option>
            ))}
          </select>
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            File
          </label>
          <div className="flex gap-2 items-center">
            <button
              onClick={handlePickFile}
              disabled={isActive}
              className="rounded bg-gray-100 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-200 disabled:opacity-50"
            >
              Choose File
            </button>
            {selectedFile && (
              <span className="text-sm text-gray-600 truncate">
                {selectedFile}
              </span>
            )}
          </div>
        </div>

        {selectedFile && selectedPeer && status === "idle" && (
          <button
            onClick={handleSend}
            className="rounded bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
          >
            Send
          </button>
        )}
      </div>

      {/* Progress */}
      {isActive && (
        <div className="bg-white rounded-lg shadow p-4 space-y-4">
          <TransferProgress progress={progressData} />
          <div className="flex gap-2">
            {status === "sending" ? (
              <button
                onClick={handlePause}
                className="rounded bg-yellow-500 px-3 py-1 text-xs font-medium text-white hover:bg-yellow-600"
              >
                Pause
              </button>
            ) : (
              <button
                onClick={handleSend}
                className="rounded bg-green-600 px-3 py-1 text-xs font-medium text-white hover:bg-green-700"
              >
                Resume
              </button>
            )}
            <button
              onClick={handleCancel}
              className="rounded bg-red-600 px-3 py-1 text-xs font-medium text-white hover:bg-red-700"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Success */}
      {status === "completed" && (
        <div className="bg-white rounded-lg shadow p-4">
          <div className="rounded bg-green-50 border border-green-200 text-green-800 px-4 py-3 text-sm">
            <p className="font-medium">File sent successfully</p>
            {completedHash && (
              <p className="text-xs mt-1 font-mono break-all">
                Hash: {completedHash}
              </p>
            )}
          </div>
        </div>
      )}

      {/* Error */}
      {status === "failed" && (
        <div className="bg-white rounded-lg shadow p-4">
          <div className="rounded bg-red-50 border border-red-200 text-red-700 px-4 py-3 text-sm">
            <p className="font-medium">Send failed</p>
            {error && <p className="text-xs mt-1">{error}</p>}
            <button
              onClick={handleRetry}
              className="mt-2 rounded bg-red-600 px-3 py-1 text-xs font-medium text-white hover:bg-red-700"
            >
              Retry
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
