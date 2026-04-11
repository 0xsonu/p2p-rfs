import { useState, useCallback, useEffect } from "react";
import {
  getLocalInfo,
  listPeers,
  connectToPeer,
  onPeerDiscovered,
  onPeerLost,
  type PeerInfo,
  type LocalInfo,
} from "../services/p2pBridge";

export function DashboardScreen() {
  const [localInfo, setLocalInfo] = useState<LocalInfo | null>(null);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [manualAddress, setManualAddress] = useState("");
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [connectError, setConnectError] = useState<string | null>(null);

  // Fetch local info and initial peer list
  useEffect(() => {
    Promise.all([getLocalInfo(), listPeers()])
      .then(([info, peerList]) => {
        setLocalInfo(info);
        setPeers(peerList);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  // Subscribe to peer discovery/lost events
  useEffect(() => {
    const unlistenDiscovered = onPeerDiscovered((peer) => {
      setPeers((prev) => {
        const idx = prev.findIndex((p) => p.id === peer.id);
        if (idx >= 0) {
          const updated = [...prev];
          updated[idx] = peer;
          return updated;
        }
        return [...prev, peer];
      });
    });

    const unlistenLost = onPeerLost((payload) => {
      setPeers((prev) => prev.filter((p) => p.id !== payload.peer_id));
    });

    return () => {
      unlistenDiscovered.then((fn) => fn());
      unlistenLost.then((fn) => fn());
    };
  }, []);

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

  const handleManualConnect = useCallback(async () => {
    const addr = manualAddress.trim();
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
      setManualAddress("");
    } catch (err) {
      const message = err instanceof Error ? err.message : "Connection failed";
      setConnectError(message);
    } finally {
      setConnectingId(null);
    }
  }, [manualAddress]);

  function statusBadge(status: string) {
    const colors: Record<string, string> = {
      Connected: "bg-green-100 text-green-800",
      Discovered: "bg-blue-100 text-blue-800",
      Unreachable: "bg-gray-100 text-gray-600",
    };
    const cls = colors[status] ?? "bg-gray-100 text-gray-600";
    return (
      <span
        className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${cls}`}
      >
        {status}
      </span>
    );
  }

  return (
    <div className="space-y-6">
      <h2 className="text-sm font-semibold text-gray-700 uppercase tracking-wide">
        Peers
      </h2>

      {/* Local device info */}
      <section className="bg-white rounded-lg shadow p-4">
        <h3 className="text-sm font-semibold text-gray-700 mb-2">
          This Device
        </h3>
        {loading ? (
          <p className="text-sm text-gray-500">Loading…</p>
        ) : localInfo ? (
          <div className="text-sm text-gray-600 space-y-1">
            <p>Name: {localInfo.display_name}</p>
            <p>Port: {localInfo.listen_port}</p>
            <p className="text-xs font-mono text-gray-400 break-all">
              Fingerprint: {localInfo.cert_fingerprint}
            </p>
          </div>
        ) : (
          <p className="text-sm text-gray-500">Unable to load local info.</p>
        )}
      </section>

      {/* Manual connection */}
      <section className="bg-white rounded-lg shadow p-4">
        <h3 className="text-sm font-semibold text-gray-700 mb-2">
          Manual Connection
        </h3>
        <div className="flex gap-2">
          <input
            type="text"
            value={manualAddress}
            onChange={(e) => setManualAddress(e.target.value)}
            placeholder="IP:port (e.g. 192.168.1.10:4433)"
            className="flex-1 rounded border border-gray-300 px-3 py-2 text-sm"
          />
          <button
            onClick={handleManualConnect}
            disabled={connectingId === "manual" || !manualAddress.trim()}
            className="rounded bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {connectingId === "manual" ? "Connecting…" : "Connect"}
          </button>
        </div>
        {connectError && (
          <p className="mt-2 text-xs text-red-600">{connectError}</p>
        )}
      </section>

      {/* Discovered peers */}
      <section className="bg-white rounded-lg shadow p-4">
        <h3 className="text-sm font-semibold text-gray-700 mb-3">
          Discovered Peers
        </h3>
        {loading ? (
          <p className="text-sm text-gray-500">Scanning network…</p>
        ) : peers.length === 0 ? (
          <p className="text-sm text-gray-500">
            No peers found on the network.
          </p>
        ) : (
          <ul className="divide-y divide-gray-200">
            {peers.map((peer) => (
              <li
                key={peer.id}
                className="flex items-center justify-between py-3"
              >
                <div className="min-w-0">
                  <p className="text-sm font-medium text-gray-900">
                    {peer.display_name}
                  </p>
                  <p className="text-xs text-gray-500">
                    {peer.addresses.length > 0
                      ? peer.addresses[0]
                      : "No address"}
                  </p>
                </div>
                <div className="flex items-center gap-3 ml-4">
                  {statusBadge(peer.status)}
                  {peer.status !== "Connected" && (
                    <button
                      onClick={() =>
                        handleConnect(peer.id, peer.addresses[0] ?? "")
                      }
                      disabled={connectingId === peer.id}
                      className="rounded bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-700 disabled:opacity-50"
                    >
                      {connectingId === peer.id ? "Connecting…" : "Connect"}
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
