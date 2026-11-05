// Typed bridge to the Rust side. Outside Tauri (browser preview) a synthetic drive is used.
import { mockInvoke } from "./mock";

export interface Drive { root: string; label: string; total: number; free: number; fs: string }
export interface ScanInfo {
  root: string; total: number; alloc: number; files: number; items: number; scan_ms: number;
  method: string; via_service: boolean; volume_total: number; volume_free: number; scanned_at: number; has_previous: boolean;
}
export interface Item { id: number; path: string; name: string; is_dir: boolean; size: number; alloc: number; files: number; mtime: number }
export interface Slice { id: number[]; parent: number[]; size: number[]; dir: number[]; name: string[]; ext: string[] }
export interface TypeStat { ext: string; size: number; files: number }
export interface Candidate { id: number; path: string; size: number; kind: string; why: string; safety: "Safe" | "Review" | "System" }
export interface Change { path: string; before: number; after: number; delta: number; is_new: boolean }
export interface Growth { since: number; now: number; total_delta: number; grown: Change[]; shrunk: Change[]; new_files: Change[] }
export interface DupeGroup { size: number; paths: string[]; wasted: number }
export interface DeleteRequest { id: string; paths: string[]; sizes: number[]; reason: string; created: number; state: string; message: string }

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (inTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mockInvoke(cmd, args) as Promise<T>;
}

export const api = {
  drives: () => call<Drive[]>("list_drives"),
  scan: (target: string) => call<ScanInfo>("scan", { target }),
  tree: (node: number, maxNodes: number) => call<Slice>("tree", { node, maxNodes }),
  children: (node: number, limit: number) => call<Item[]>("children", { node, limit }),
  item: (node: number) => call<Item>("item", { node }),
  ancestors: (node: number) => call<Item[]>("ancestors", { node }),
  topFiles: (node: number, limit: number) => call<Item[]>("top_files", { node, limit }),
  hotspots: (node: number, limit: number) => call<Item[]>("hotspots", { node, limit }),
  types: (node: number, limit: number) => call<TypeStat[]>("file_types", { node, limit }),
  search: (node: number, pattern: string, limit: number) => call<Item[]>("search", { node, pattern, limit }),
  cleanup: (node: number) => call<Candidate[]>("cleanup_candidates", { node }),
  growth: () => call<Growth | null>("growth"),
  duplicates: (node: number, minMb: number) => call<DupeGroup[]>("duplicates", { node, minMb }),
  find: (path: string) => call<number | null>("find", { path }),
  reveal: (path: string) => call<void>("reveal", { path }),
  recycle: (paths: string[]) => call<void>("recycle", { paths }),
  pending: () => call<DeleteRequest[]>("pending_requests"),
  resolve: (id: string, approve: boolean) => call<DeleteRequest>("resolve_request", { id, approve }),
};

export async function windowAction(a: "minimize" | "toggleMaximize" | "close" | "drag") {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const w = getCurrentWindow();
  if (a === "minimize") await w.minimize();
  else if (a === "toggleMaximize") await w.toggleMaximize();
  else if (a === "close") await w.close();
  else await w.startDragging();
}
