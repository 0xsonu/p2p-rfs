import { useState, useCallback, useEffect } from "react";
import {
  getTransferHistory,
  type TransferHistoryEntry,
} from "../services/p2pBridge";

/**
 * Sort transfer history entries in descending chronological order by timestamp.
 * Exported as a pure function for property-based testing.
 */
export function sortHistory(
  entries: TransferHistoryEntry[],
): TransferHistoryEntry[] {
  return [...entries].sort(
    (a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime(),
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function formatTimestamp(ts: string): string {
  return new Date(ts).toLocaleString();
}

function statusBadge(status: string) {
  const styles: Record<string, string> = {
    success: "bg-green-100 text-green-800",
    failed: "bg-red-100 text-red-800",
  };
  const style = styles[status] ?? "bg-gray-100 text-gray-800";
  return (
    <span
      className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${style}`}
    >
      {status}
    </span>
  );
}

function directionLabel(direction: string) {
  return direction === "sent" ? "↑ Sent" : "↓ Received";
}

export function HistoryScreen() {
  const [entries, setEntries] = useState<TransferHistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedFailure, setSelectedFailure] =
    useState<TransferHistoryEntry | null>(null);

  useEffect(() => {
    getTransferHistory()
      .then((data) => setEntries(data))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const sorted = sortHistory(entries);

  const handleSelectFailed = useCallback((entry: TransferHistoryEntry) => {
    if (entry.status === "failed") {
      setSelectedFailure((prev) =>
        prev?.session_id === entry.session_id ? null : entry,
      );
    }
  }, []);

  return (
    <div className="space-y-6">
      <h2 className="text-sm font-semibold text-gray-700 uppercase tracking-wide">
        Transfer History
      </h2>

      <div className="bg-white rounded-lg shadow">
        {loading ? (
          <p className="p-4 text-sm text-gray-500">Loading history…</p>
        ) : sorted.length === 0 ? (
          <p className="p-4 text-sm text-gray-500">No transfer history.</p>
        ) : (
          <ul className="divide-y divide-gray-100">
            {sorted.map((entry) => (
              <li
                key={entry.session_id}
                className={`p-4 ${entry.status === "failed" ? "cursor-pointer hover:bg-gray-50" : ""}`}
                onClick={() => handleSelectFailed(entry)}
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <span className="text-xs text-gray-500 w-20">
                      {directionLabel(entry.direction)}
                    </span>
                    <span className="text-sm font-medium text-gray-900">
                      {entry.file_name}
                    </span>
                  </div>
                  <div className="flex items-center gap-4">
                    <span className="text-xs text-gray-500">
                      {formatBytes(entry.file_size)}
                    </span>
                    {statusBadge(entry.status)}
                  </div>
                </div>
                <div className="mt-1 flex items-center gap-4 text-xs text-gray-400">
                  <span>{formatTimestamp(entry.timestamp)}</span>
                  <span className="text-gray-400">
                    Peer: {entry.peer_display_name}
                  </span>
                </div>

                {/* Failure detail */}
                {selectedFailure?.session_id === entry.session_id && (
                  <div className="mt-2 rounded border border-red-200 bg-red-50 p-3">
                    <p className="text-sm text-red-800 font-medium">
                      Transfer Failed
                    </p>
                    {entry.failure_reason && (
                      <p className="text-xs text-red-600 mt-1">
                        Reason: {entry.failure_reason}
                      </p>
                    )}
                    <p className="text-xs text-red-600">
                      {formatTimestamp(entry.timestamp)}
                    </p>
                  </div>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
