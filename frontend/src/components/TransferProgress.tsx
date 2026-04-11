import type { TransferProgress as ProgressData } from "../hooks/useTransfer";

interface TransferProgressProps {
  progress: ProgressData;
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec >= 1_000_000) {
    return `${(bytesPerSec / 1_000_000).toFixed(1)} MB/s`;
  }
  if (bytesPerSec >= 1_000) {
    return `${(bytesPerSec / 1_000).toFixed(1)} KB/s`;
  }
  return `${Math.round(bytesPerSec)} B/s`;
}

function formatEta(seconds: number): string {
  if (seconds <= 0) return "—";
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  if (seconds < 3600)
    return `${Math.floor(seconds / 60)}m ${Math.ceil(seconds % 60)}s`;
  const h = Math.floor(seconds / 3600);
  const m = Math.ceil((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

/**
 * Shared progress bar component displaying percentage, speed, ETA, and chunk progress.
 * Used by both UploadScreen and DownloadScreen.
 */
export function TransferProgress({ progress }: TransferProgressProps) {
  const pct = Math.min(100, Math.max(0, progress.percentage));

  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-xs text-gray-600">
        <span>{pct.toFixed(1)}%</span>
        <span>
          {progress.completedChunks}/{progress.totalChunks} chunks
        </span>
      </div>

      <div
        className="w-full h-2 bg-gray-200 rounded-full overflow-hidden"
        role="progressbar"
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div
          className="h-full bg-blue-600 rounded-full transition-all duration-200"
          style={{ width: `${pct}%` }}
        />
      </div>

      <div className="flex items-center justify-between text-xs text-gray-500">
        <span>{formatSpeed(progress.speed)}</span>
        <span>ETA: {formatEta(progress.eta)}</span>
      </div>
    </div>
  );
}
