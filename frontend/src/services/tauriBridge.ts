/**
 * Tauri IPC bridge — provides file dialog and filesystem access.
 *
 * Uses direct imports from the installed @tauri-apps/plugin-dialog package.
 */

import { open, save } from "@tauri-apps/plugin-dialog";

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

/** Open a native file-picker dialog. Returns selected file paths. */
export async function openFileDialog(
  options: OpenDialogOptions = {},
): Promise<string[]> {
  const result = await open({
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
  const result = await save({
    defaultPath: options.defaultPath,
    filters: options.filters,
    title: options.title,
  });
  return result ?? null;
}
