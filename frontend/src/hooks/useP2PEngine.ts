
import { useCallback, useEffect, useState } from "react";
import { type EngineInfo, startEngine } from "../services/p2pBridge";


export type EngineStatus = "stopped" | "starting" | "running" | "error";

export interface UseP2PEngineReturn {
  status: EngineStatus;
  engineInfo: EngineInfo | null;
  error: string | null;
  restart: () => void;
}


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
    } catch (err: unknown) {
      let message = "Failed to start P2P engine";
      if (err instanceof Error) {
        message = err.message;
      } else if (
        typeof err == "object" && err != null && "message" in err &&
        typeof (err as Record<string, unknown>).message === "string"
      ) {
        const cmdErr = err as { code?: string; message: string };
        message = cmdErr.code ? `[${cmdErr.code}] ${cmdErr.message}` : cmdErr.message;
      } else if (typeof err === "string") {
        message = err;
      }

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

