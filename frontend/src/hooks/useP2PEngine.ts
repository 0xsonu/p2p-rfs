import { useState, useEffect, useCallback } from "react";
import { startEngine, type EngineInfo } from "../services/p2pBridge";

export type EngineStatus = "stopped" | "starting" | "running" | "error";

export interface UseP2PEngineReturn {
  status: EngineStatus;
  engineInfo: EngineInfo | null;
  error: string | null;
  restart: () => void;
}

/**
 * Hook that starts the P2P engine on mount and tracks its status.
 * Replaces the old useAuth hook — no login/logout in P2P mode.
 */
export function useP2PEngine(): UseP2PEngineReturn {
  const [status, setStatus] = useState<EngineStatus>("stopped");
  const [engineInfo, setEngineInfo] = useState<EngineInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  const init = useCallback(async () => {
    setStatus("starting");
    setError(null);
    try {
      const info = await startEngine();
      setEngineInfo(info);
      setStatus("running");
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to start P2P engine";
      setError(message);
      setStatus("error");
    }
  }, []);

  useEffect(() => {
    init();
  }, [init, attempt]);

  const restart = useCallback(() => {
    setAttempt((prev) => prev + 1);
  }, []);

  return { status, engineInfo, error, restart };
}
