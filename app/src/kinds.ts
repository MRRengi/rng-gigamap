// File categories and their colors (muted Apple system hues, tuned per theme).

export type Kind = "video" | "image" | "audio" | "archive" | "game" | "code" | "doc" | "app" | "system" | "other" | "folder";

const MAP: Record<string, Kind> = {};
const reg = (k: Kind, exts: string) => exts.split(" ").forEach((e) => (MAP[e] = k));
reg("video", "mp4 mkv mov avi webm wmv m4v flv bk2 usm");
reg("image", "jpg jpeg png gif webp heic raw cr2 nef psd tga tif tiff bmp exr hdr svg ico dds ktx");
reg("audio", "mp3 wav ogg flac m4a aac wma bank fsb");
reg("archive", "zip 7z rar tar gz tgz xz zst iso img vhd vhdx wim esd cab msi msp unitypackage nupkg");
reg("game", "pak bin bundle assets resource ress fbx blend obj uasset umap gltf glb vpk");
reg("code", "cs js ts tsx jsx rs py cpp c h hpp java kt go json xml yaml yml toml html css map lua sh ps1 rlib rmeta o pdb d");
reg("doc", "pdf doc docx xls xlsx ppt pptx txt md rtf odt csv epub");
reg("app", "exe dll sys mui so dylib node wasm appx msix");
reg("system", "tmp log dmp etl manifest cat dat db sqlite ldb data info");

export function kindOf(ext: string, isDir = false): Kind {
  if (isDir) return "folder";
  return MAP[ext] ?? "other";
}

const LIGHT: Record<Kind, string> = {
  video: "#f0645a", image: "#f6a33b", audio: "#ec5f9b", archive: "#9b72f2", game: "#6f74f0",
  code: "#33b884", doc: "#e8c33a", app: "#4cb3e6", system: "#9aa1ad", other: "#b7b9c2", folder: "#8e8e96",
};
const DARK: Record<Kind, string> = {
  video: "#e8574e", image: "#e8962e", audio: "#dd5590", archive: "#8d66e6", game: "#6267e3",
  code: "#2aa676", doc: "#d4b02c", app: "#3fa2d4", system: "#7d838f", other: "#8f919a", folder: "#6e6e76",
};
const dark = () => window.matchMedia("(prefers-color-scheme: dark)").matches;
export const color = (k: Kind) => (dark() ? DARK : LIGHT)[k];

export const LABEL: Record<Kind, string> = {
  video: "Video", image: "Images", audio: "Audio", archive: "Archives & disks", game: "Game data",
  code: "Code & build", doc: "Documents", app: "Apps", system: "System", other: "Other", folder: "Folder",
};
export const LEGEND: Kind[] = ["video", "image", "audio", "archive", "game", "code", "doc", "app", "system"];

// Lucide-style outline icons (24px grid, 1.75 stroke).
const P: Record<string, string> = {
  drive: '<path d="M22 12H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><path d="M6 16h.01"/><path d="M10 16h.01"/>',
  folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  file: '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  refresh: '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
  chevron: '<path d="m9 18 6-6-6-6"/>',
  trash: '<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><path d="M10 11v6"/><path d="M14 11v6"/>',
  reveal: '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  copy: '<rect width="14" height="14" x="8" y="8" rx="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  sparkles: '<path d="M9.94 14.06 8 20l-1.94-5.94L0 12l6.06-1.94L8 4l1.94 6.06L16 12Z" transform="translate(2 -1) scale(0.9)"/><path d="M20 3v4"/><path d="M22 5h-4"/>',
  trend: '<path d="M22 7 13.5 15.5 8.5 10.5 2 17"/><path d="M16 7h6v6"/>',
  copies: '<path d="M20 7h-3a2 2 0 0 1-2-2V2"/><path d="M9 18a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h7l4 4v10a2 2 0 0 1-2 2Z"/><path d="M3 7.6v12.8A1.6 1.6 0 0 0 4.6 22h9.8"/>',
  pie: '<path d="M21.21 15.89A10 10 0 1 1 8 2.83"/><path d="M22 12A10 10 0 0 0 12 2v10z"/>',
  list: '<path d="M3 12h.01"/><path d="M3 18h.01"/><path d="M3 6h.01"/><path d="M8 12h13"/><path d="M8 18h13"/><path d="M8 6h13"/>',
  x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
  minus: '<path d="M5 12h14"/>',
  square: '<rect width="16" height="16" x="4" y="4" rx="1"/>',
  bolt: '<path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/>',
  turtle: '<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>',
  bot: '<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>',
  up: '<path d="m18 15-6-6-6 6"/>',
  back: '<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>',
  film: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M7 3v18"/><path d="M3 7.5h4"/><path d="M3 12h18"/><path d="M3 16.5h4"/><path d="M17 3v18"/><path d="M17 7.5h4"/><path d="M17 16.5h4"/>',
  image: '<rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.09-3.09a2 2 0 0 0-2.82 0L6 21"/>',
  music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  box: '<path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"/><path d="m3.3 7 8.7 5 8.7-5"/><path d="M12 22V12"/>',
  gamepad: '<path d="M6 11h4"/><path d="M8 9v4"/><path d="M15 12h.01"/><path d="M18 10h.01"/><path d="M17.32 5H6.68a4 4 0 0 0-3.98 3.59c-.01.05-.01.1-.02.15C2.6 9.42 2 14.46 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.41-1.41A2 2 0 0 1 9.83 16h4.34a2 2 0 0 1 1.41.59L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.55-.6-6.58-.68-7.26-.01-.05-.01-.1-.02-.15A4 4 0 0 0 17.32 5z"/>',
  code: '<path d="m16 18 6-6-6-6"/><path d="m8 6-6 6 6 6"/>',
  doc: '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>',
  app: '<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="14" y="3" rx="1"/><rect width="7" height="7" x="14" y="14" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/>',
  gear: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
};
export const icon = (name: string) => `<svg class="i" viewBox="0 0 24 24" aria-hidden="true">${P[name] ?? P.file}</svg>`;
export const KIND_ICON: Record<Kind, string> = {
  video: "film", image: "image", audio: "music", archive: "box", game: "gamepad", code: "code",
  doc: "doc", app: "app", system: "gear", other: "file", folder: "folder",
};

export function size(b: number): string {
  const u = ["B", "KB", "MB", "GB", "TB", "PB"];
  if (b < 1024) return `${Math.round(b)} B`;
  let v = b, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2)} ${u[i]}`;
}
export function pct(p: number, w: number): string {
  if (!w || p <= 0) return "0%";
  const v = (p * 100) / w;
  return v < 0.1 ? "<0.1%" : v >= 10 ? `${v.toFixed(0)}%` : `${v.toFixed(1)}%`;
}
export const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
