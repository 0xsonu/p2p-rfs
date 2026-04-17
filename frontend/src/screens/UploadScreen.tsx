import { useCallback } from "react";
import { openFileDialog } from "../services/tauriBridge";
import { useAppState } from "../hooks/useAppState";
import { TransferProgress } from "../components/TransferProgress";

export function UploadScreen() {
  const {
    peers,
    send,
    setSendSelectedPeer,
    setSendSelectedFile,
    handleSend,
    handleSendPause,
    handleSendCancel,
    handleSendRetry,
  } = useAppState();

  const connectedPeers = peers.filter((p) => p.status === "Connected");

  const handlePickFile = useCallback(async () => {
    const paths = await openFileDialog({
      multiple: false,
      title: "Select file to send",
    });
    if (paths.length > 0) {
      setSendSelectedFile(paths[0]);
    }
  }, [setSendSelectedFile]);

  const isActive = send.status === "sending" || send.status === "paused";

  const progressData = send.progress
    ? {
        percentage: send.progress.percentage,
        speed: send.progress.speed_bps,
        eta: send.progress.eta_seconds,
        completedChunks: send.progress.completed_chunks,
        totalChunks: send.progress.total_chunks,
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
            value={send.selectedPeer}
            onChange={(e) => setSendSelectedPeer(e.target.value)}
            disabled={isActive}
            className="w-full rounded border border-gray-300 px-3 py-2 text-sm"
          >
            <option value="">Select a connected peer…</option>
            {connectedPeers.map((p) => (
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
            {send.selectedFile && (
              <span className="text-sm text-gray-600 truncate">
                {send.selectedFile}
              </span>
            )}
          </div>
        </div>

        {send.selectedFile && send.selectedPeer && send.status === "idle" && (
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
            {send.status === "sending" ? (
              <button
                onClick={handleSendPause}
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
              onClick={handleSendCancel}
              className="rounded bg-red-600 px-3 py-1 text-xs font-medium text-white hover:bg-red-700"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Success */}
      {send.status === "completed" && (
        <div className="bg-white rounded-lg shadow p-4">
          <div className="rounded bg-green-50 border border-green-200 text-green-800 px-4 py-3 text-sm">
            <p className="font-medium">File sent successfully</p>
            {send.completedHash && (
              <p className="text-xs mt-1 font-mono break-all">
                Hash: {send.completedHash}
              </p>
            )}
          </div>
        </div>
      )}

      {/* Error */}
      {send.status === "failed" && (
        <div className="bg-white rounded-lg shadow p-4">
          <div className="rounded bg-red-50 border border-red-200 text-red-700 px-4 py-3 text-sm">
            <p className="font-medium">Send failed</p>
            {send.error && <p className="text-xs mt-1">{send.error}</p>}
            <button
              onClick={handleSendRetry}
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
