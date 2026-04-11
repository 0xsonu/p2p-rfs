import { useState, useCallback } from "react";

export type TransferStatus =
  | "idle"
  | "uploading"
  | "downloading"
  | "paused"
  | "completed"
  | "failed";

export interface TransferProgress {
  percentage: number;
  speed: number; // bytes/sec
  eta: number; // seconds remaining
  completedChunks: number;
  totalChunks: number;
}

export interface TransferState {
  status: TransferStatus;
  progress: TransferProgress;
  error: string | null;
  fileHash: string | null;
  integrityResult: string | null;
}

export interface UseTransferReturn extends TransferState {
  start: (direction: "uploading" | "downloading", totalChunks: number) => void;
  pause: () => void;
  resume: () => void;
  cancel: () => void;
  recordChunk: (bytesTransferred: number, elapsedTime: number) => void;
  complete: (hash: string, integrityResult?: string) => void;
  fail: (error: string) => void;
}

/**
 * Pure function to compute transfer progress from transfer state.
 * Exported for property-based testing (Property 22).
 *
 * **Validates: Requirements 14.2, 15.2**
 */
export function computeProgress(
  totalChunks: number,
  completedChunks: number,
  elapsedTime: number,
  bytesTransferred: number,
): TransferProgress {
  const percentage =
    totalChunks > 0 ? (completedChunks / totalChunks) * 100 : 0;
  const speed = elapsedTime > 0 ? bytesTransferred / elapsedTime : 0;
  const remainingChunks = totalChunks - completedChunks;
  const timePerChunk = completedChunks > 0 ? elapsedTime / completedChunks : 0;
  const eta = Math.max(0, remainingChunks * timePerChunk);

  return { percentage, speed, eta, completedChunks, totalChunks };
}

const INITIAL_PROGRESS: TransferProgress = {
  percentage: 0,
  speed: 0,
  eta: 0,
  completedChunks: 0,
  totalChunks: 0,
};

/**
 * Hook for managing transfer state (upload or download).
 * Provides start, pause, resume, cancel, and progress tracking.
 */
export function useTransfer(): UseTransferReturn {
  const [status, setStatus] = useState<TransferStatus>("idle");
  const [progress, setProgress] = useState<TransferProgress>(INITIAL_PROGRESS);
  const [error, setError] = useState<string | null>(null);
  const [fileHash, setFileHash] = useState<string | null>(null);
  const [integrityResult, setIntegrityResult] = useState<string | null>(null);

  const start = useCallback(
    (direction: "uploading" | "downloading", totalChunks: number) => {
      setStatus(direction);
      setProgress({ ...INITIAL_PROGRESS, totalChunks });
      setError(null);
      setFileHash(null);
      setIntegrityResult(null);
    },
    [],
  );

  const pause = useCallback(() => {
    setStatus("paused");
  }, []);

  const resume = useCallback(() => {
    setStatus((prev) => (prev === "paused" ? "uploading" : prev));
  }, []);

  const cancel = useCallback(() => {
    setStatus("idle");
    setProgress(INITIAL_PROGRESS);
    setError(null);
  }, []);

  const recordChunk = useCallback(
    (bytesTransferred: number, elapsedTime: number) => {
      setProgress((prev) => {
        const completed = prev.completedChunks + 1;
        return computeProgress(
          prev.totalChunks,
          completed,
          elapsedTime,
          bytesTransferred,
        );
      });
    },
    [],
  );

  const complete = useCallback((hash: string, integrity?: string) => {
    setStatus("completed");
    setFileHash(hash);
    setIntegrityResult(integrity ?? "verified");
  }, []);

  const fail = useCallback((msg: string) => {
    setStatus("failed");
    setError(msg);
  }, []);

  return {
    status,
    progress,
    error,
    fileHash,
    integrityResult,
    start,
    pause,
    resume,
    cancel,
    recordChunk,
    complete,
    fail,
  };
}
