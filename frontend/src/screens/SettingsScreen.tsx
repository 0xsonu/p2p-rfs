import { useState, useCallback, useEffect } from "react";
import {
  getSettings,
  saveSettings,
  type P2PSettings,
} from "../services/p2pBridge";

/**
 * Validation error for a specific settings field.
 */
export interface ValidationError {
  field: string;
  reason: string;
}

/**
 * Validate P2P settings. Returns all validation errors.
 * Exported as a pure function for property-based testing.
 */
export function validateSettings(settings: P2PSettings): ValidationError[] {
  const errors: ValidationError[] = [];

  if (!settings.display_name || settings.display_name.trim().length === 0) {
    errors.push({
      field: "display_name",
      reason: "must not be empty",
    });
  }

  if (
    !Number.isInteger(settings.listen_port) ||
    settings.listen_port < 1 ||
    settings.listen_port > 65535
  ) {
    errors.push({
      field: "listen_port",
      reason: "must be between 1 and 65535",
    });
  }

  if (!Number.isInteger(settings.chunk_size) || settings.chunk_size <= 0) {
    errors.push({
      field: "chunk_size",
      reason: "must be a positive integer",
    });
  }

  if (
    !Number.isInteger(settings.parallel_streams) ||
    settings.parallel_streams <= 0
  ) {
    errors.push({
      field: "parallel_streams",
      reason: "must be a positive integer",
    });
  }

  return errors;
}

const DEFAULT_SETTINGS: P2PSettings = {
  display_name: "",
  listen_port: 4433,
  chunk_size: 4194304,
  parallel_streams: 4,
  per_transfer_rate_limit: 0,
  download_dir: "",
};

export function SettingsScreen() {
  const [settings, setSettings] = useState<P2PSettings>(DEFAULT_SETTINGS);
  const [errors, setErrors] = useState<ValidationError[]>([]);
  const [saved, setSaved] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => setSettings(s))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const fieldError = useCallback(
    (field: string) => errors.find((e) => e.field === field),
    [errors],
  );

  const handleChange = useCallback(
    (field: keyof P2PSettings, value: string) => {
      setSaved(false);
      setSaveError(null);
      setSettings((prev) => {
        if (field === "display_name" || field === "download_dir") {
          return { ...prev, [field]: value };
        }
        const num = parseInt(value, 10);
        return { ...prev, [field]: isNaN(num) ? 0 : num };
      });
    },
    [],
  );

  const handleSave = useCallback(async () => {
    const validationErrors = validateSettings(settings);
    setErrors(validationErrors);
    if (validationErrors.length > 0) return;

    setSaveError(null);
    try {
      await saveSettings(settings);
      setSaved(true);
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to save settings";
      setSaveError(message);
    }
  }, [settings]);

  if (loading) {
    return (
      <div className="space-y-6">
        <h2 className="text-sm font-semibold text-gray-700 uppercase tracking-wide">
          Settings
        </h2>
        <p className="text-sm text-gray-500">Loading settings…</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <h2 className="text-sm font-semibold text-gray-700 uppercase tracking-wide">
        Settings
      </h2>

      <div className="bg-white rounded-lg shadow p-6 space-y-5">
        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Display Name
          </label>
          <input
            type="text"
            value={settings.display_name}
            onChange={(e) => handleChange("display_name", e.target.value)}
            className={`w-full rounded border px-3 py-2 text-sm ${
              fieldError("display_name")
                ? "border-red-400 bg-red-50"
                : "border-gray-300"
            }`}
          />
          {fieldError("display_name") && (
            <p className="mt-1 text-xs text-red-600">
              {fieldError("display_name")!.reason}
            </p>
          )}
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            QUIC Listener Port
          </label>
          <input
            type="number"
            value={settings.listen_port}
            onChange={(e) => handleChange("listen_port", e.target.value)}
            className={`w-full rounded border px-3 py-2 text-sm ${
              fieldError("listen_port")
                ? "border-red-400 bg-red-50"
                : "border-gray-300"
            }`}
          />
          {fieldError("listen_port") && (
            <p className="mt-1 text-xs text-red-600">
              {fieldError("listen_port")!.reason}
            </p>
          )}
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Chunk Size (bytes)
          </label>
          <input
            type="number"
            value={settings.chunk_size}
            onChange={(e) => handleChange("chunk_size", e.target.value)}
            className={`w-full rounded border px-3 py-2 text-sm ${
              fieldError("chunk_size")
                ? "border-red-400 bg-red-50"
                : "border-gray-300"
            }`}
          />
          {fieldError("chunk_size") && (
            <p className="mt-1 text-xs text-red-600">
              {fieldError("chunk_size")!.reason}
            </p>
          )}
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Parallel Streams
          </label>
          <input
            type="number"
            value={settings.parallel_streams}
            onChange={(e) => handleChange("parallel_streams", e.target.value)}
            className={`w-full rounded border px-3 py-2 text-sm ${
              fieldError("parallel_streams")
                ? "border-red-400 bg-red-50"
                : "border-gray-300"
            }`}
          />
          {fieldError("parallel_streams") && (
            <p className="mt-1 text-xs text-red-600">
              {fieldError("parallel_streams")!.reason}
            </p>
          )}
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Per-Transfer Rate Limit (bytes/sec, 0 = unlimited)
          </label>
          <input
            type="number"
            value={settings.per_transfer_rate_limit}
            onChange={(e) =>
              handleChange("per_transfer_rate_limit", e.target.value)
            }
            className="w-full rounded border border-gray-300 px-3 py-2 text-sm"
          />
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Download Directory
          </label>
          <input
            type="text"
            value={settings.download_dir}
            onChange={(e) => handleChange("download_dir", e.target.value)}
            placeholder="/path/to/downloads"
            className="w-full rounded border border-gray-300 px-3 py-2 text-sm"
          />
        </div>

        <div className="flex items-center gap-3 pt-2">
          <button
            onClick={handleSave}
            className="rounded bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
          >
            Save Settings
          </button>
          {saved && (
            <span className="text-sm text-green-600 font-medium">
              Settings saved.
            </span>
          )}
          {saveError && (
            <span className="text-sm text-red-600 font-medium">
              {saveError}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
