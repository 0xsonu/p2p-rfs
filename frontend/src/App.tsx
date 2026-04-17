import { useState } from "react";
import "./App.css";
import { useP2PEngine } from "./hooks/useP2PEngine";
import { AppStateProvider } from "./providers/AppStateProvider";
import { DashboardScreen } from "./screens/DashboardScreen";
import { UploadScreen } from "./screens/UploadScreen";
import { DownloadScreen } from "./screens/DownloadScreen";
import { HistoryScreen } from "./screens/HistoryScreen";
import { SettingsScreen } from "./screens/SettingsScreen";

type Screen = "peers" | "send" | "receive" | "history" | "settings";

function NavBar({
  current,
  onNavigate,
}: {
  current: Screen;
  onNavigate: (s: Screen) => void;
}) {
  const items: { key: Screen; label: string }[] = [
    { key: "peers", label: "Peers" },
    { key: "send", label: "Send" },
    { key: "receive", label: "Receive" },
    { key: "history", label: "History" },
    { key: "settings", label: "Settings" },
  ];

  return (
    <nav className="bg-white border-b border-gray-200">
      <div className="mx-auto max-w-4xl flex items-center justify-between px-6 py-2">
        <div className="flex items-center gap-1">
          {items.map((item) => (
            <button
              key={item.key}
              onClick={() => onNavigate(item.key)}
              className={`px-3 py-1.5 text-sm font-medium rounded ${
                current === item.key
                  ? "bg-blue-100 text-blue-700"
                  : "text-gray-600 hover:text-gray-900 hover:bg-gray-100"
              }`}
            >
              {item.label}
            </button>
          ))}
        </div>
      </div>
    </nav>
  );
}

/**
 * Screen content rendered inside the provider.
 * All screens stay mounted (hidden via CSS) so their state is never lost.
 */
function AppContent() {
  const [screen, setScreen] = useState<Screen>("peers");

  return (
    <div className="min-h-screen bg-gray-50">
      <NavBar current={screen} onNavigate={setScreen} />
      <div className="mx-auto max-w-4xl px-6 py-6">
        <div style={{ display: screen === "peers" ? "block" : "none" }}>
          <DashboardScreen />
        </div>
        <div style={{ display: screen === "send" ? "block" : "none" }}>
          <UploadScreen />
        </div>
        <div style={{ display: screen === "receive" ? "block" : "none" }}>
          <DownloadScreen />
        </div>
        <div style={{ display: screen === "history" ? "block" : "none" }}>
          <HistoryScreen />
        </div>
        <div style={{ display: screen === "settings" ? "block" : "none" }}>
          <SettingsScreen />
        </div>
      </div>
    </div>
  );
}

function App() {
  const engine = useP2PEngine();

  if (engine.status === "starting") {
    return (
      <div className="min-h-screen bg-gray-50 flex items-center justify-center">
        <p className="text-sm text-gray-500">Starting P2P engine…</p>
      </div>
    );
  }

  if (engine.status === "error") {
    return (
      <div className="min-h-screen bg-gray-50 flex items-center justify-center">
        <div className="bg-white rounded-lg shadow p-6 max-w-sm text-center space-y-3">
          <p className="text-sm text-red-600 font-medium">
            Failed to start P2P engine
          </p>
          {engine.error && (
            <p className="text-xs text-gray-500">{engine.error}</p>
          )}
          <button
            onClick={engine.restart}
            className="rounded bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
          >
            Retry
          </button>
        </div>
      </div>
    );
  }

  return (
    <AppStateProvider>
      <AppContent />
    </AppStateProvider>
  );
}

export default App;
