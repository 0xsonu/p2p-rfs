import { useState, useCallback, useEffect } from "react";
import { saveFileDialog } from "../services/tauriBridge";
import {
  acceptTransfer,
  rejectTransfer,
  onIncomingTransfer,
  onTransferProgress,
  onTransferComplete,
  onTransferFailed,
  type IncomingTransferPayload,
  type TransferProgressPayload,
} from "../services/p2pBridge";
import { TransferProgress } from "../components/TransferProgress";

interface IncomingRequest extends IncomingTransferPayload {
  status: "pending" | "accepted" | "rejected";
}

interface ActiveReceive {
  sessionId: string;
  fileName: string;
  progress: TransferProgressPayload | null;
  status: "receiving" | "completed" | "failed";
  hash?: string;
  error?: string;
}

function formatSize(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  if (bytes >= 1_000) return `${(bytes / 1_000).toFixed(1)} KB`;
  return `${bytes} B`;
}

/**
 * Receive screen — listen for incoming transfer requests, accept/reject,
 * choose save location, and display receive progress.
 *
 * Requirements: 6.1, 12.1, 12.2, 16.1–16.5
 */
export function DownloadScreen() {
  const [incoming, setIncoming] = useState<IncomingRequest[]>([]);
  const [activeReceives, setActiveReceives] = useState<ActiveReceive[]>([]);

  // Subscribe to incoming transfer requests
  useEffect(() => {
    const unIncoming = onIncomingTransfer((payload) => {
      setIncoming((prev) => [...prev, { ...payload, status: "pending" }]);
    });
    return () => {
      unIncoming.then((fn) => fn());
    };
  }, []);

  // Subscribe to progress/complete/failed events for active receives
  useEffect(() => {
    const unProgress = onTransferProgress((p) => {
      setActiveReceives((prev) =>
        prev.map((r) =>
          r.sessionId === p.session_id ? { ...r, progress: p } : r,
        ),
      );
    });
    const unComplete = onTransferComplete((p) => {
      setActiveReceives((prev) =>
        prev.map((r) =>
          r.sessionId === p.session_id
            ? { ...r, status: "completed", hash: p.hash }
            : r,
        ),
      );
    });
    const unFailed = onTransferFailed((p) => {
      setActiveReceives((prev) =>
        prev.map((r) =>
          r.sessionId === p.session_id
            ? { ...r, status: "failed", error: p.reason }
            : r,
        ),
      );
    });
    return () => {
      unProgress.then((fn) => fn());
      unComplete.then((fn) => fn());
      unFailed.then((fn) => fn());
    };
  }, []);

  const handleAccept = useCallback(async (req: IncomingRequest) => {
    const savePath = await saveFileDialog({
      defaultPath: req.file_name,
      title: "Save received file as",
    });
    if (!savePath) return; // user cancelled

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
  }, []);

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

  const pendingRequests = incoming.filter((r) => r.status === "pending");

  return (
    <div className="space-y-6">
      <h2 className="text-sm font-semibold text-gray-700 uppercase tracking-wide">
        Receive Files
      </h2>

      {/* Incoming transfer requests */}
      {pendingRequests.length > 0 && (
        <section className="space-y-3">
          {pendingRequests.map((req) => (
            <div
              key={req.session_id}
              className="bg-white rounded-lg shadow p-4 border-l-4 border-blue-500"
            >
              <p className="text-sm font-medium text-gray-900">
                Incoming file from {req.sender_name}
              </p>
              <p className="text-xs text-gray-500 mt-1">
                {req.file_name} · {formatSize(req.file_size)}
              </p>
              <div className="flex gap-2 mt-3">
                <button
                  onClick={() => handleAccept(req)}
                  className="rounded bg-green-600 px-3 py-1 text-xs font-medium text-white hover:bg-green-700"
                >
                  Accept
                </button>
                <button
                  onClick={() => handleReject(req.session_id)}
                  className="rounded bg-red-600 px-3 py-1 text-xs font-medium text-white hover:bg-red-700"
                >
                  Reject
                </button>
              </div>
            </div>
          ))}
        </section>
      )}

      {/* Active receives */}
      {activeReceives.map((recv) => (
        <div
          key={recv.sessionId}
          className="bg-white rounded-lg shadow p-4 space-y-3"
        >
          <p className="text-sm font-medium text-gray-900">
            {recv.status === "receiving"
              ? "Receiving"
              : recv.status === "completed"
                ? "Received"
                : "Failed"}
            : {recv.fileName}
          </p>

          {recv.status === "receiving" && recv.progress && (
            <TransferProgress
              progress={{
                percentage: recv.progress.percentage,
                speed: recv.progress.speed_bps,
                eta: recv.progress.eta_seconds,
                completedChunks: recv.progress.completed_chunks,
                totalChunks: recv.progress.total_chunks,
              }}
            />
          )}

          {recv.status === "completed" && (
            <div className="rounded bg-green-50 border border-green-200 text-green-800 px-4 py-3 text-sm">
              <p className="font-medium">File received successfully</p>
              {recv.hash && (
                <p className="text-xs mt-1 font-mono break-all">
                  Hash: {recv.hash}
                </p>
              )}
            </div>
          )}

          {recv.status === "failed" && (
            <div className="rounded bg-red-50 border border-red-200 text-red-700 px-4 py-3 text-sm">
              <p className="font-medium">Receive failed</p>
              {recv.error && <p className="text-xs mt-1">{recv.error}</p>}
            </div>
          )}
        </div>
      ))}

      {/* Empty state */}
      {pendingRequests.length === 0 && activeReceives.length === 0 && (
        <div className="bg-white rounded-lg shadow p-4">
          <p className="text-sm text-gray-500">
            Waiting for incoming transfer requests from peers…
          </p>
        </div>
      )}
    </div>
  );
}
