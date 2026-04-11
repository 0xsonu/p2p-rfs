/**
 * Tauri IPC bridge — provides file dialog and filesystem access.
 *
 * When running inside Tauri the real APIs are used; in a plain browser
 * environment the functions fall back to stubs / browser equivalents so
 * the app can still be developed and tested without the Tauri runtime.
 *
 * Tauri plugin packages are loaded dynamically at runtime so the frontend
 * can build and run without them installed as npm dependencies.
 */

/** Whether we are running inside a Tauri desktop shell. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

export interface OpenDialogOptions {
  multiple?: boolean;
  directory?: boolean;
  filters?: { name: string; extensions: string[] }[];
  title?: string;
}

export interface SaveDialogOptions {
  defaultPath?: string;
  filters?: { name: string; extensions: string[] }[];
  title?: string;
}

/**
 * Dynamically import a Tauri plugin module.
 * Returns `undefined` when not running inside Tauri.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function tauriImport(module: string): Promise<any | undefined> {
  if (!isTauri()) return undefined;
  // Dynamic import — the module string is only resolved at runtime inside Tauri
  return import(/* @vite-ignore */ module);
}

/** Open a native file-picker dialog. Returns selected file paths. */
export async function openFileDialog(
  options: OpenDialogOptions = {},
): Promise<string[]> {
  const mod = await tauriImport("@tauri-apps/plugin-dialog");
  if (!mod) {
    console.warn(
      "[tauriBridge] openFileDialog called outside Tauri — returning empty",
    );
    return [];
  }
  const result = await mod.open({
    multiple: options.multiple ?? false,
    directory: options.directory ?? false,
    filters: options.filters,
    title: options.title,
  });
  if (result === null) return [];
  return Array.isArray(result) ? result : [result];
}

/** Open a native save-file dialog. Returns the chosen path or null. */
export async function saveFileDialog(
  options: SaveDialogOptions = {},
): Promise<string | null> {
  const mod = await tauriImport("@tauri-apps/plugin-dialog");
  if (!mod) {
    console.warn(
      "[tauriBridge] saveFileDialog called outside Tauri — returning null",
    );
    return null;
  }
  const result = await mod.save({
    defaultPath: options.defaultPath,
    filters: options.filters,
    title: options.title,
  });
  return result ?? null;
}

/** Read a file from the local filesystem (Tauri only). */
export async function readBinaryFile(path: string): Promise<Uint8Array> {
  const mod = await tauriImport("@tauri-apps/plugin-fs");
  if (!mod) throw new Error("readBinaryFile is only available inside Tauri");
  return mod.readFile(path);
}

/** Write binary data to a file on the local filesystem (Tauri only). */
export async function writeBinaryFile(
  path: string,
  data: Uint8Array,
): Promise<void> {
  const mod = await tauriImport("@tauri-apps/plugin-fs");
  if (!mod) throw new Error("writeBinaryFile is only available inside Tauri");
  await mod.writeFile(path, data);
}
