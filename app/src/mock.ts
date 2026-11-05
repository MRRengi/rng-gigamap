// Synthetic drive for the browser preview only (never used inside the app).
// Mirrors the Rust command contract closely enough to exercise every screen.

type N = { parent: number; name: string; size: number; dir: boolean; ext: string; mtime: number; kids: number[]; total: number; files: number };

let rnd = 42;
const r = () => ((rnd = (rnd * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
const pick = <T,>(a: T[]) => a[Math.floor(r() * a.length)];
const GB = 1024 ** 3, MB = 1024 ** 2;

const nodes: N[] = [];
function add(parent: number, name: string, dir: boolean, size = 0, mtime = 1_760_000_000): number {
  const ext = dir ? "" : (name.includes(".") ? name.split(".").pop()!.toLowerCase() : "");
  nodes.push({ parent, name, size, dir, ext, mtime, kids: [], total: 0, files: 0 });
  const id = nodes.length - 1;
  if (parent >= 0) nodes[parent].kids.push(id);
  return id;
}
const d = (p: number, n: string) => add(p, n, true);
const f = (p: number, n: string, s: number, m?: number) => add(p, n, false, Math.round(s), m);
function spray(p: number, count: number, exts: string[], avg: number, prefix = "file") {
  for (let i = 0; i < count; i++) f(p, `${prefix}_${i}.${pick(exts)}`, avg * (0.1 + r() * r() * 4));
}

const root = d(-1, "");
const users = d(root, "Users"), maks = d(users, "Maks");
const appdata = d(maks, "AppData"), local = d(appdata, "Local"), roaming = d(appdata, "Roaming");
const temp = d(local, "Temp"); spray(temp, 400, ["tmp", "log", "dmp", "zip"], 6 * MB);
const chrome = d(d(d(local, "Google"), "Chrome"), "User Data"), prof = d(chrome, "Default");
f(prof, "Preferences", 40_000); spray(d(prof, "Cache"), 900, ["data", "bin"], 2.4 * MB, "f");
spray(d(prof, "Code Cache"), 300, ["js", "wasm"], 900_000, "c");
const unityHub = d(d(local, "Unity"), "cache"); spray(unityHub, 120, ["unitypackage", "tgz"], 60 * MB, "pkg");
const npm = d(local, "npm-cache"); spray(d(npm, "_cacache"), 1200, ["bin"], 700_000, "content");
spray(d(roaming, "Telegram Desktop"), 160, ["jpg", "mp4", "webp"], 9 * MB, "media");
const docs = d(maks, "Documents");
spray(d(docs, "Contracts"), 60, ["pdf", "docx"], 1.2 * MB, "contract");
const proj = d(maks, "Projects");
for (const name of ["Sticks & Stones", "Godly Survivors", "Yesday", "Blind Faith", "Martin RTS"]) {
  const p = d(proj, name);
  d(p, "ProjectSettings"); const assets = d(p, "Assets");
  spray(d(assets, "Art"), 260, ["png", "psd", "fbx", "tga"], 7 * MB, "art");
  spray(d(assets, "Audio"), 90, ["wav", "ogg"], 11 * MB, "sfx");
  spray(d(assets, "Scripts"), 340, ["cs"], 14_000, "Script");
  const lib = d(p, "Library");
  spray(d(lib, "Artifacts"), 1400, ["bin", "info"], 3.1 * MB, "a");
  spray(d(lib, "PackageCache"), 300, ["dll", "cs", "json"], 1.4 * MB, "p");
  spray(d(p, "Temp"), 40, ["tmp"], 4 * MB);
  spray(d(p, "Builds"), 6, ["zip", "exe"], 900 * MB, "build");
}
const web = d(proj, "rengi.dev"); f(web, "package.json", 2_000); spray(d(web, "node_modules"), 2600, ["js", "map", "d.ts", "json"], 160_000, "m");
const rust = d(proj, "rng-gigamap"); f(rust, "Cargo.toml", 600); spray(d(rust, "target"), 900, ["rlib", "rmeta", "o"], 2.6 * MB, "dep");
const vids = d(maks, "Videos"); spray(d(vids, "Captures"), 70, ["mp4"], 1.6 * GB, "Recording");
const dl = d(maks, "Downloads"); spray(dl, 140, ["exe", "zip", "msi", "iso", "pdf", "mp4"], 220 * MB, "download");
f(dl, "Win11_24H2.iso", 5.6 * GB); f(dl, "Win11_24H2 (1).iso", 5.6 * GB);
const pics = d(maks, "Pictures"); spray(pics, 900, ["jpg", "heic", "png", "raw"], 5 * MB, "IMG");
const pf = d(root, "Program Files");
for (const app of ["Unity", "JetBrains", "Blender Foundation", "Adobe", "NVIDIA Corporation", "Microsoft Office"]) spray(d(pf, app), 380, ["dll", "exe", "pak", "dat"], 9 * MB, "lib");
const steam = d(d(d(root, "Program Files (x86)"), "Steam"), "steamapps");
const common = d(steam, "common");
for (const g of ["Baldur's Gate 3", "Cyberpunk 2077", "Elden Ring", "Warcraft III"]) spray(d(common, g), 180, ["pak", "bin", "bk2", "dll"], 420 * MB, "data");
const win = d(root, "Windows");
spray(d(win, "System32"), 2400, ["dll", "exe", "sys", "mui"], 1.1 * MB, "sys");
spray(d(win, "WinSxS"), 3000, ["dll", "manifest", "cat"], 2.4 * MB, "x");
spray(d(d(win, "SoftwareDistribution"), "Download"), 80, ["cab", "psf", "esd"], 60 * MB, "upd");
spray(d(win, "Installer"), 140, ["msi", "msp"], 55 * MB, "pkg");
f(root, "pagefile.sys", 24 * GB); f(root, "hiberfil.sys", 12.7 * GB); f(root, "swapfile.sys", 256 * MB);
spray(d(root, "$Recycle.Bin"), 30, ["zip", "mp4"], 120 * MB, "$R");

(function finalize() {
  for (let i = nodes.length - 1; i >= 0; i--) {
    const n = nodes[i];
    n.total += n.dir ? 0 : n.size; n.files += n.dir ? 0 : 1;
    if (n.parent >= 0) { nodes[n.parent].total += n.total; nodes[n.parent].files += n.files; }
  }
  for (const n of nodes) n.kids.sort((a, b) => nodes[b].total - nodes[a].total);
})();
const volume = 1024 * GB;

function path(i: number): string {
  const parts: string[] = [];
  while (i > 0) { parts.push(nodes[i].name); i = nodes[i].parent; }
  return "C:\\" + parts.reverse().join("\\");
}
const item = (i: number) => ({ id: i, path: path(i), name: nodes[i].name || "C:\\", is_dir: nodes[i].dir, size: nodes[i].total, alloc: nodes[i].total, files: nodes[i].files, mtime: nodes[i].mtime });
function* sub(i: number): Generator<number> { const st = [i]; while (st.length) { const c = st.pop()!; yield c; st.push(...nodes[c].kids); } }

function slice(n: number, max: number) {
  const s = { id: [] as number[], parent: [] as number[], size: [] as number[], dir: [] as number[], name: [] as string[], ext: [] as string[] };
  const push = (k: number, p: number) => { s.id.push(k); s.parent.push(p); s.size.push(nodes[k].total); s.dir.push(nodes[k].dir ? 1 : 0); s.name.push(nodes[k].name); s.ext.push(nodes[k].ext); };
  push(n, -1);
  const heap: [number, number, number][] = nodes[n].kids.map((k) => [nodes[k].total, k, 0]);
  while (heap.length && s.id.length < max) {
    heap.sort((a, b) => a[0] - b[0]);
    const [size, k, p] = heap.pop()!;
    if (size <= 0) break;
    const me = s.id.length; push(k, p);
    for (const c of nodes[k].kids) heap.push([nodes[c].total, c, me]);
  }
  return s;
}

let pendingDemo: any[] = [];
setTimeout(() => {
  const a = nodes.findIndex((n) => n.name === "target");
  const b = nodes.findIndex((n) => n.name === "node_modules");
  pendingDemo = [{ id: "demo-1", paths: [path(a), path(b)], sizes: [nodes[a].total, nodes[b].total], reason: "Build output and packages, both regenerate on the next build.", created: Date.now() / 1000, state: "pending", message: "" }];
}, 2600);

export async function mockInvoke(cmd: string, a: Record<string, any>): Promise<unknown> {
  await new Promise((res) => setTimeout(res, cmd === "scan" ? 900 : 10));
  const n = (a.node ?? 0) as number;
  switch (cmd) {
    case "list_drives": return [
      { root: "C:\\", label: "Windows", total: volume, free: volume - nodes[0].total - 38 * GB, fs: "NTFS" },
      { root: "D:\\", label: "Projects", total: 2048 * GB, free: 1210 * GB, fs: "NTFS" },
      { root: "E:\\", label: "SD card", total: 256 * GB, free: 201 * GB, fs: "exFAT" },
    ];
    case "scan": return { root: "C:\\", total: nodes[0].total, alloc: nodes[0].total, files: nodes[0].files, items: nodes.length, scan_ms: 1840, method: "Mft", via_service: true, volume_total: volume, volume_free: volume - nodes[0].total - 38 * GB, scanned_at: Date.now() / 1000, has_previous: true };
    case "tree": return slice(n, a.maxNodes);
    case "children": return nodes[n].kids.slice(0, a.limit).map(item);
    case "item": return item(n);
    case "ancestors": { const v = []; let i = n; while (true) { v.push(item(i)); if (i === 0) break; i = nodes[i].parent; } return v.reverse(); }
    case "top_files": return [...sub(n)].filter((i) => !nodes[i].dir).sort((x, y) => nodes[y].size - nodes[x].size).slice(0, a.limit).map(item);
    case "hotspots": return [...sub(n)].filter((i) => i !== n && nodes[i].dir && nodes[i].kids.every((k) => !nodes[k].dir)).sort((x, y) => nodes[y].total - nodes[x].total).slice(0, a.limit).map(item);
    case "file_types": { const m = new Map<string, { size: number; files: number }>(); for (const i of sub(n)) if (!nodes[i].dir) { const e = m.get(nodes[i].ext) ?? { size: 0, files: 0 }; e.size += nodes[i].size; e.files++; m.set(nodes[i].ext, e); } return [...m].map(([ext, v]) => ({ ext, ...v })).sort((x, y) => y.size - x.size).slice(0, a.limit); }
    case "search": { const q = String(a.pattern).toLowerCase().replace(/\*/g, ""); return [...sub(n)].filter((i) => i && nodes[i].name.toLowerCase().includes(q)).sort((x, y) => nodes[y].total - nodes[x].total).slice(0, a.limit).map(item); }
    case "cleanup_candidates": {
      const rules: [string, string, string, string][] = [["Library", "Unity Library", "Unity rebuilds it on project open", "Safe"], ["node_modules", "Node packages", "reinstalled by npm install", "Safe"], ["target", "Rust target", "cargo build output", "Safe"], ["Cache", "Browser cache", "browser re-downloads it", "Safe"], ["_cacache", "npm cache", "re-downloaded on demand", "Safe"], ["Temp", "Temp files", "temporary files", "Review"], ["Download", "Windows Update cache", "old update packages", "Review"], ["Builds", "Unity builds", "old player builds", "Review"], ["$Recycle.Bin", "Recycle Bin", "already deleted files", "Review"], ["hiberfil.sys", "Hibernation file", "managed by Windows (powercfg /h off)", "System"], ["pagefile.sys", "Page file", "managed by Windows", "System"]];
      const out = []; for (const i of sub(n)) { const rr = rules.find((x) => x[0] === nodes[i].name); if (rr) out.push({ id: i, path: path(i), size: nodes[i].total, kind: rr[1], why: rr[2], safety: rr[3] }); }
      return out.sort((x, y) => y.size - x.size);
    }
    case "growth": {
      const ids = ["Captures", "Artifacts", "Cache", "Temp"].map((nm) => nodes.findIndex((x) => x.name === nm));
      const grown = ids.map((i, k) => ({ path: path(i), before: nodes[i].total * (0.55 + k * 0.1), after: nodes[i].total, delta: nodes[i].total * (0.45 - k * 0.1), is_new: false }));
      return { since: Date.now() / 1000 - 86400 * 3, now: Date.now() / 1000, total_delta: grown.reduce((s, g) => s + g.delta, 0) - 3.1 * GB, grown, shrunk: [{ path: path(nodes.findIndex((x) => x.name === "Downloads")), before: 40 * GB, after: 36.9 * GB, delta: -3.1 * GB, is_new: false }], new_files: [...sub(vids)].filter((i) => !nodes[i].dir).slice(0, 4).map((i) => ({ path: path(i), before: 0, after: nodes[i].size, delta: nodes[i].size, is_new: true })) };
    }
    case "duplicates": return [{ size: 5.6 * GB, wasted: 5.6 * GB, paths: ["C:\\Users\\Maks\\Downloads\\Win11_24H2.iso", "C:\\Users\\Maks\\Downloads\\Win11_24H2 (1).iso"] }];
    case "find": return null;
    case "pending_requests": return pendingDemo;
    case "resolve_request": pendingDemo = []; return { state: a.approve ? "done" : "declined" };
    default: return null;
  }
}
