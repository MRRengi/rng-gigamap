import { api, inTauri, pickFolder, windowAction, type Drive, type Item, type ScanInfo, type Slice, type DeleteRequest } from "./api";
import { Treemap } from "./treemap";
import { color, esc, icon, kindOf, KIND_ICON, LABEL, LEGEND, pct, size } from "./kinds";

type Tab = "largest" | "types" | "growth" | "cleanup" | "dupes" | "search";
type Since = "last" | "day" | "week" | "month";

const S = {
  drives: [] as Drive[],
  lastScan: new Map<string, number>(),
  info: null as ScanInfo | null,
  target: "",
  node: 0,
  slice: null as Slice | null,
  selected: null as Item | null,
  folder: null as Item | null,
  tab: "largest" as Tab,
  scanning: false,
  query: "",
  queryWhole: false,
  typeExt: null as string | null,
  since: "last" as Since,
  dupesMin: 50,
  dupesRun: false,
};

/** Rescan in the background when the snapshot we opened is older than this. */
const STALE_SECS = 10 * 60;

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector(sel) as T;

/** Escaped path that wraps at separators instead of mid-name. */
const pathHtml = (p: string) => esc(p).replace(/\\/g, "\\<wbr>");

function ago(ts: number): string {
  const s = Math.max(0, Date.now() / 1000 - ts);
  if (s < 90) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  const d = Math.round(s / 86400);
  return d === 1 ? "yesterday" : `${d} days ago`;
}

function shell() {
  if (!inTauri) document.body.classList.add("preview");
  $("#app").innerHTML = `
  <div class="shell">
    <aside class="sidebar">
      <div class="sidebar-drag" data-drag>
        <div class="brand"><span class="brand-mark"><span></span><span></span><span></span></span>Gigamap</div>
      </div>
      <div class="side-section">
        <div class="side-title">Drives</div>
        <div id="drives"></div>
        <button class="side-row side-folder" id="pick">${icon("folder")}<div class="drive-meta"><div class="drive-name">Map a folder…</div><div class="drive-sub">Any folder or network share</div></div></button>
      </div>
      <div id="mode" class="side-foot"></div>
    </aside>
    <main class="main">
      <header class="titlebar" data-drag>
        <nav class="crumbs" id="crumbs" aria-label="Location"></nav>
        <div class="tool-group">
          <span class="refresh-chip" id="refresh" hidden><i></i>Refreshing</span>
          <label class="search">${icon("search")}<input id="q" placeholder="Search names" spellcheck="false" /><kbd>Ctrl F</kbd></label>
          <button class="btn" id="up" title="Up one level (Backspace, Alt+Left or the mouse back button)">${icon("back")}Up</button>
          <button class="btn" id="rescan" title="Rescan (F5)">${icon("refresh")}Rescan</button>
        </div>
        <div class="caption">
          <button data-win="minimize" aria-label="Minimize">${icon("minus")}</button>
          <button data-win="toggleMaximize" aria-label="Maximize">${icon("square")}</button>
          <button data-win="close" class="close" aria-label="Close">${icon("x")}</button>
        </div>
      </header>
      <div class="workspace">
        <section class="center">
          <div class="map-wrap" id="map">
            <div class="map-tip" id="tip"></div>
            <div id="mapstate"></div>
          </div>
          <div class="map-legend" id="legend"></div>
          <div class="panel">
            <div class="panel-head">
              <div class="seg" id="tabs" role="tablist">
                <span class="knob"></span>
                <button data-tab="largest" role="tab">${icon("list")}Largest</button>
                <button data-tab="types" role="tab">${icon("pie")}Types</button>
                <button data-tab="growth" role="tab">${icon("trend")}Growth</button>
                <button data-tab="cleanup" role="tab">${icon("sparkles")}Cleanup</button>
                <button data-tab="dupes" role="tab">${icon("copies")}Duplicates</button>
              </div>
              <div class="panel-tools" id="tools"></div>
              <div class="panel-note" id="note"></div>
            </div>
            <div class="list" id="list"></div>
          </div>
        </section>
        <aside class="inspector" id="insp"></aside>
      </div>
    </main>
  </div>
  <div class="toasts" id="toasts"></div>
  <div class="menu" id="menu" role="menu" hidden></div>
  <div class="modal-bg" id="modal" hidden></div>
  ${inTauri ? "" : `<div class="preview-badge">Browser preview with a synthetic drive</div>`}`;

  document.querySelectorAll<HTMLElement>("[data-drag]").forEach((el) =>
    el.addEventListener("mousedown", (e) => {
      if ((e.target as HTMLElement).closest("button,input,label,.refresh-chip")) return;
      if (e.detail === 2) windowAction("toggleMaximize");
      else windowAction("drag");
    }),
  );
  document.querySelectorAll<HTMLButtonElement>("[data-win]").forEach((b) =>
    b.addEventListener("click", () => windowAction(b.dataset.win as "minimize")),
  );
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((b) =>
    b.addEventListener("click", () => setTab(b.dataset.tab as Tab)),
  );
  $("#legend").innerHTML = LEGEND.map((k) => `<span class="chip"><i style="background:${color(k)}"></i>${LABEL[k]}</span>`).join("");
  $("#up").addEventListener("click", up);
  $("#rescan").addEventListener("click", () => S.target && scan(S.target, { background: true }));
  $("#pick").addEventListener("click", async () => {
    const dir = await pickFolder();
    if (dir) choose(dir);
  });
  let t = 0;
  $<HTMLInputElement>("#q").addEventListener("input", (e) => {
    clearTimeout(t);
    const v = (e.target as HTMLInputElement).value.trim();
    t = window.setTimeout(() => {
      S.query = v;
      S.queryWhole = false;
      if (v.length >= 2) setTab("search");
      else if (S.tab === "search") setTab("largest");
    }, 200);
  });
  window.addEventListener("keydown", keys);
  window.addEventListener("mouseup", (e) => {
    if (e.button === 3) { e.preventDefault(); up(); }
  });
  window.addEventListener("mousedown", (e) => {
    if (!(e.target as HTMLElement).closest("#menu")) closeMenu();
  });
  window.addEventListener("blur", closeMenu);
  window.addEventListener("resize", closeMenu);
}

// ---------- treemap ----------
let map: Treemap;
function setupMap() {
  map = new Treemap($("#map"));
  const tip = $("#tip");
  map.onHover = (i, x, y) => {
    if (i < 0 || !S.slice) return tip.classList.remove("show");
    const s = S.slice;
    const where = map.trail(i);
    tip.innerHTML = `<div class="t-name">${esc(s.name[i])}${s.dir[i] ? "\\" : ""}</div>
      ${where ? `<div class="t-path">${esc(where)}</div>` : ""}
      <div class="t-sub num">${size(s.size[i])} · ${pct(s.size[i], s.size[0] || 1)} of this view</div>`;
    const box = $("#map").getBoundingClientRect();
    const tw = tip.offsetWidth, th = tip.offsetHeight;
    tip.style.left = `${Math.min(x + 14, box.width - tw - 8)}px`;
    tip.style.top = `${y + th + 22 > box.height ? y - th - 10 : y + 18}px`;
    tip.classList.add("show");
  };
  map.onSelect = async (i) => {
    if (!S.slice) return;
    map.select(i);
    S.selected = await api.item(S.slice.id[i]);
    inspector();
  };
  map.onOpen = (i) => S.slice && open(S.slice.id[i], map.rectOf(i));
  map.onContext = async (i, x, y) => {
    if (!S.slice) return;
    $("#tip").classList.remove("show");
    const it = await api.item(S.slice.id[i]);
    map.select(i);
    S.selected = it;
    inspector();
    menuFor(it, x, y);
  };
}

// ---------- data flow ----------
async function loadDrives() {
  S.drives = await api.drives();
  try {
    const ts = await api.lastScans(S.drives.map((d) => d.root));
    ts.forEach((t, k) => t && S.lastScan.set(S.drives[k].root, t));
  } catch { /* history is optional */ }
  drives();
  if (!S.info) picker();
}

async function refreshDrives() {
  try {
    S.drives = await api.drives();
    drives();
  } catch { /* keep the old numbers */ }
}

/** Pick a drive or folder: show the last saved scan instantly, refresh it if stale. */
async function choose(target: string) {
  if (S.scanning) return;
  closeMenu();
  let snap: ScanInfo | null = null;
  try { snap = await api.openLast(target); } catch { snap = null; }
  if (snap) {
    S.target = target;
    S.info = snap;
    $("#mapstate").innerHTML = "";
    $("#legend").style.opacity = "1";
    drives();
    mode();
    await open(0);
    if (Date.now() / 1000 - snap.scanned_at > STALE_SECS) scan(target, { background: true });
  } else {
    scan(target);
  }
}

let scanTimer = 0;
async function scan(target: string, opts: { background?: boolean } = {}) {
  if (S.scanning) return;
  const keepPath = opts.background && S.target === target ? S.folder?.path ?? null : null;
  const keepSel = opts.background && S.target === target ? S.selected?.path ?? null : null;
  const background = !!opts.background && S.target === target && !!S.info;
  S.target = target;
  S.scanning = true;
  drives();
  syncUp();
  const started = Date.now();
  if (background) {
    $("#refresh").hidden = false;
  } else {
    const label = esc(target.replace(/\\$/, ""));
    $("#mapstate").innerHTML = `<div class="map-status"><div><div class="big">Mapping ${label}</div><div id="scan-sub">Reading the file table</div><div class="scanning-bar"><i></i></div><div class="scan-time num" id="scan-time">0.0 s</div></div></div>`;
    $("#legend").style.opacity = "0";
    clearInterval(scanTimer);
    scanTimer = window.setInterval(() => {
      const el = document.getElementById("scan-time");
      if (el) el.textContent = `${((Date.now() - started) / 1000).toFixed(1)} s`;
      const sub = document.getElementById("scan-sub");
      if (sub && Date.now() - started > 8000) sub.textContent = "Walking folders one by one. The Gigamap service makes this take seconds.";
    }, 100);
  }
  try {
    S.info = await api.scan(target);
  } catch (e) {
    clearInterval(scanTimer);
    S.scanning = false;
    $("#refresh").hidden = true;
    drives();
    syncUp();
    if (background) return notify(`Refresh failed: ${String(e)}`);
    $("#mapstate").innerHTML = `<div class="map-status"><div><div class="big">Could not read ${esc(target)}</div><div>${esc(String(e))}</div>
      <div style="margin-top:14px"><button class="btn" id="retry">${icon("refresh")}Try again</button></div></div></div>`;
    $("#retry").addEventListener("click", () => scan(target));
    return;
  }
  clearInterval(scanTimer);
  S.scanning = false;
  $("#refresh").hidden = true;
  $("#mapstate").innerHTML = "";
  $("#legend").style.opacity = "1";
  S.lastScan.set(target, S.info.scanned_at);
  refreshDrives();
  mode();
  const back = keepPath ? await api.find(keepPath) : null;
  await open(back ?? 0, undefined, false, true);
  if (keepSel && keepSel !== keepPath) focusPath(keepSel);
}

function syncUp() {
  const b = document.querySelector<HTMLButtonElement>("#up");
  if (b) b.disabled = !S.info || S.node === 0;
  const r = document.querySelector<HTMLButtonElement>("#rescan");
  if (r) r.disabled = !S.target || S.scanning;
  document.querySelector(".panel")?.classList.toggle("is-empty", !S.info);
}

let openSeq = 0;
async function open(node: number, from?: { x: number; y: number; w: number; h: number }, reverse = false, quiet = false) {
  const seq = ++openSeq;
  const [slice, trail] = await Promise.all([api.tree(node, 24000), api.ancestors(node)]);
  if (seq !== openSeq) return;
  if (from && !quiet) await map.zoomFrom(from, reverse);
  S.node = node;
  S.slice = slice;
  S.folder = trail[trail.length - 1];
  S.selected = null;
  map.set(slice);
  crumbs(trail);
  syncUp();
  inspector();
  panel();
}

async function up() {
  if (!S.folder || S.node === 0) return;
  const trail = await api.ancestors(S.node);
  const parent = trail[trail.length - 2];
  if (parent) open(parent.id);
}

async function focusPath(path: string) {
  const id = await api.find(path);
  if (id == null) return;
  S.selected = await api.item(id);
  const local = S.slice ? S.slice.id.indexOf(id) : -1;
  map.select(local);
  inspector();
}

/** Open the folder that contains a path and select it there. */
async function revealInMap(path: string) {
  const id = await api.find(path);
  if (id == null) return notify("That item is gone. Rescan to update the map.");
  const it = await api.item(id);
  if (it.is_dir) return open(id);
  const trail = await api.ancestors(id);
  await open(trail[trail.length - 2].id);
  focusPath(path);
}

// ---------- views ----------
function driveSub(d: Drive, busy: boolean) {
  if (busy) return "Scanning…";
  const last = S.lastScan.get(d.root);
  const space = d.total ? `${size(d.free)} free of ${size(d.total)}` : esc(d.fs);
  return last ? `${space} · scanned ${ago(last)}` : space;
}

function drives() {
  $("#drives").innerHTML = S.drives
    .map((d) => {
      const used = d.total ? (d.total - d.free) / d.total : 0;
      const on = S.target === d.root;
      return `<button class="side-row ${on ? "on" : ""}" data-root="${esc(d.root)}">
        ${icon("drive")}
        <div class="drive-meta">
          <div class="drive-name">${esc(d.label || "Local Disk")} (${esc(d.root.replace(/\\$/, ""))})</div>
          <div class="drive-bar"><i class="${used > 0.95 ? "full" : used > 0.85 ? "high" : ""}" style="width:${(used * 100).toFixed(1)}%"></i></div>
          <div class="drive-sub num">${driveSub(d, on && S.scanning)}</div>
        </div></button>`;
    })
    .join("");
  document.querySelectorAll<HTMLButtonElement>("#drives [data-root]").forEach((b) =>
    b.addEventListener("click", () => {
      if (b.dataset.root === S.target && S.info) return open(0);
      choose(b.dataset.root!);
    }),
  );
  const isFolder = S.target && !S.drives.some((d) => d.root === S.target);
  $("#pick").classList.toggle("on", !!isFolder);
  const sub = $("#pick").querySelector(".drive-sub");
  if (sub) sub.textContent = isFolder ? S.target : "Any folder or network share";
}

function mode() {
  const el = $("#mode");
  const i = S.info;
  if (!i) return (el.innerHTML = "");
  const fast = i.method === "Mft";
  const when = i.from_snapshot ? `Saved scan from ${ago(i.scanned_at)}.` : "";
  const skipped = i.skipped ? ` ${i.skipped.toLocaleString()} folders could not be opened (access denied) and are not counted.` : "";
  el.className = `side-foot ${fast ? "fast" : ""}`;
  el.innerHTML = fast
    ? `${icon("bolt")}<div><b>Fast mode</b>${i.items.toLocaleString()} items in ${(i.scan_ms / 1000).toFixed(1)} s, read from the NTFS file table. ${when}</div>`
    : `${icon("turtle")}<div><b>Standard mode</b>Folders were walked one by one.${skipped} ${when}</div>`;
}

function picker() {
  $("#crumbs").innerHTML = "";
  $("#legend").style.opacity = "0";
  $("#mapstate").innerHTML = `<div class="picker">
    <h1>What is filling your disk?</h1>
    <p>Pick a drive. Saved scans open instantly and refresh in the background.</p>
    <div class="picker-list">${S.drives
      .map((d) => {
        const used = d.total ? (d.total - d.free) / d.total : 0;
        const last = S.lastScan.get(d.root);
        return `<button class="picker-row" data-root="${esc(d.root)}">${icon("drive")}
        <div class="drive-meta"><div class="drive-name">${esc(d.label || "Local Disk")} (${esc(d.root.replace(/\\$/, ""))})</div>
        <div class="drive-bar"><i class="${used > 0.95 ? "full" : used > 0.85 ? "high" : ""}" style="width:${(used * 100).toFixed(1)}%"></i></div>
        <div class="drive-sub num">${size(d.total - d.free)} used · ${size(d.free)} free · ${esc(d.fs)}</div></div>
        <span class="picker-last">${last ? `Scanned ${ago(last)}` : "Not scanned yet"}</span>
        <span class="go">${icon("chevron")}</span></button>`;
      })
      .join("")}</div></div>`;
  document.querySelectorAll<HTMLButtonElement>(".picker [data-root]").forEach((b) =>
    b.addEventListener("click", () => choose(b.dataset.root!)),
  );
  $("#insp").innerHTML = `<div class="empty" style="margin-top:40px"><b>Nothing selected</b>Choose a drive to see what takes the space.</div>`;
  $("#list").innerHTML = "";
  $("#tools").innerHTML = "";
}

function crumbs(full: Item[]) {
  const sep = `<svg class="i crumb-sep" viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>`;
  const trail = full.length > 5 ? [full[0], null, ...full.slice(-3)] : full;
  $("#crumbs").innerHTML = trail
    .map((it, k) => it === null ? `${sep}<span class="crumb-more" title="${esc(full.slice(1, -3).map((x) => x.name).join(" › "))}">…</span>` : `${k ? sep : ""}<button class="crumb" data-id="${it.id}">${esc(k === 0 ? it.path.replace(/\\$/, "") : it.name)}</button>`)
    .join("");
  document.querySelectorAll<HTMLButtonElement>(".crumb").forEach((b) =>
    b.addEventListener("click", () => {
      const id = Number(b.dataset.id);
      if (id !== S.node) open(id, { x: 0, y: 0, w: 0, h: 0 }, true);
    }),
  );
}

const mark = (name: string, dir: boolean) =>
  dir ? `<span class="fold">${icon("folder")}</span>` : `<span class="swatch" style="background:${rowIcon({ name, is_dir: false })}"></span>`;

function rowIcon(it: { name: string; is_dir: boolean }) {
  const ext = it.name.includes(".") ? it.name.split(".").pop()!.toLowerCase() : "";
  return color(kindOf(ext, it.is_dir));
}

let inspSeq = 0;
async function inspector() {
  const it = S.selected ?? S.folder;
  if (!it || !S.info) return;
  const seq = ++inspSeq;
  const kind = kindOf(it.name.split(".").pop()!.toLowerCase(), it.is_dir);
  const shareOf = S.selected ? (S.folder?.id === 0 ? "this drive" : S.folder?.name || "this folder") : "the volume";
  const shareTotal = S.selected ? (S.folder?.size || S.info.total) : (S.info.volume_total || S.info.total);
  const isRoot = it.id === 0;
  const LIMIT = 60;
  const [kids, parentName] = await Promise.all([
    it.is_dir ? api.children(it.id, LIMIT) : Promise.resolve([] as Item[]),
    S.selected ? Promise.resolve(S.folder?.id === 0 ? S.folder.path.replace(/\\$/, "") : S.folder?.name ?? "") : parentLabel(),
  ]);
  if (seq !== inspSeq) return;
  const date = it.mtime ? new Date(it.mtime * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }) : "—";
  const showBack = !!S.selected || S.node !== 0;
  const shownSize = kids.reduce((s, k) => s + k.size, 0);
  const more = it.is_dir && kids.length >= LIMIT ? it.size - shownSize : 0;
  $("#insp").innerHTML = `
    ${showBack ? `<button class="insp-back" data-act="back">${icon("back")}<span>${esc(parentName || "Back")}</span></button>` : ""}
    <div class="insp-head">
      <div class="insp-icon" style="background:${it.is_dir ? "var(--accent)" : color(kind)}">${icon(KIND_ICON[kind])}</div>
      <div class="insp-title"><b>${esc(isRoot ? it.path : it.name)}</b><span>${it.is_dir ? "Folder" : LABEL[kind]}</span></div>
    </div>
    <div>
      <div class="insp-size num">${size(it.size)}<small>${pct(it.size, shareTotal)} of ${esc(shareOf)}</small></div>
      <div class="share" style="margin-top:10px"><i style="width:${Math.min(100, (it.size * 100) / (shareTotal || 1)).toFixed(2)}%"></i></div>
    </div>
    <dl class="facts">
      ${it.is_dir ? `<dt>Files</dt><dd class="num">${it.files.toLocaleString()}</dd>` : ""}
      <dt>On disk</dt><dd class="num">${size(it.alloc)}</dd>
      <dt>Modified</dt><dd class="num">${date}</dd>
      ${isRoot && S.info.volume_total ? `<dt>Free</dt><dd class="num">${size(S.info.volume_free)} of ${size(S.info.volume_total)}</dd>` : ""}
    </dl>
    <div class="actions">
      ${S.selected && S.selected.is_dir ? `<button class="btn primary" data-act="open">${icon("folder")}Open in map</button>` : ""}
      <button class="btn ghost" data-act="reveal">${icon("reveal")}Show in Explorer</button>
      <button class="btn ghost" data-act="copy">${icon("copy")}Copy path</button>
      ${isRoot ? "" : `<button class="btn ghost danger-text" data-act="ask">${icon("trash")}Move to Recycle Bin…</button>`}
    </div>
    ${kids.length ? `<div class="insp-list-title">Inside</div><div class="insp-list">${showBack && !S.selected ? `<div class="row up-row" data-act="back"><span class="fold">${icon("back")}</span><div class="name"><b>..</b></div><div class="size"></div></div>` : ""}${kids
      .map((k) => `<div class="row" data-path="${esc(k.path)}" data-dir="${k.is_dir ? 1 : 0}" data-id="${k.id}" ${k.is_dir ? 'data-nav="1"' : ""}>
        ${mark(k.name, k.is_dir)}
        <div class="name"><b>${esc(k.name)}</b></div>
        <div class="size">${size(k.size)}</div></div>`)
      .join("")}${more > 0 ? `<div class="more-row num">and more items · ${size(more)}</div>` : ""}</div>` : ""}`;
  $("#insp").querySelectorAll<HTMLElement>("[data-act]").forEach((b) =>
    b.addEventListener("click", async () => {
      const a = b.dataset.act;
      if (a === "back") {
        if (S.selected) { S.selected = null; map.select(-1); inspector(); }
        else up();
        return;
      }
      if (a === "open" && S.selected) open(S.selected.id, S.slice ? map.rectOf(S.slice.id.indexOf(S.selected.id)) : undefined);
      if (a === "reveal") reveal(it.path);
      if (a === "copy") copyPath(it.path);
      if (a === "ask") confirmRecycle([{ path: it.path, size: it.size, name: it.name }]);
    }),
  );
  $("#insp").querySelectorAll<HTMLElement>(".row[data-nav]").forEach((r) =>
    r.addEventListener("click", (e) => {
      e.stopImmediatePropagation();
      open(Number(r.dataset.id));
    }, true),
  );
  bindRows($("#insp"));
}

async function parentLabel(): Promise<string> {
  if (S.node === 0) return "";
  const trail = await api.ancestors(S.node);
  const p = trail[trail.length - 2];
  return p ? (p.id === 0 ? p.path.replace(/\\$/, "") : p.name) : "";
}

function bindRows(scope: HTMLElement) {
  scope.querySelectorAll<HTMLElement>(".row[data-path]").forEach((r) => {
    r.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).closest("[data-rowact]")) return;
      scope.querySelectorAll(".row.sel").forEach((x) => x.classList.remove("sel"));
      r.classList.add("sel");
      focusPath(r.dataset.path!);
    });
    r.addEventListener("dblclick", (e) => {
      if ((e.target as HTMLElement).closest("[data-rowact]")) return;
      revealInMap(r.dataset.path!);
    });
    r.addEventListener("contextmenu", async (e) => {
      e.preventDefault();
      const id = await api.find(r.dataset.path!);
      if (id == null) return notify("That item is gone. Rescan to update the map.");
      menuFor(await api.item(id), e.clientX, e.clientY);
    });
  });
  scope.querySelectorAll<HTMLElement>("[data-rowact='trash']").forEach((b) =>
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      const r = b.closest<HTMLElement>(".row")!;
      confirmRecycle([{ path: r.dataset.path!, size: Number(r.dataset.size || 0), name: r.dataset.name || r.dataset.path! }]);
    }),
  );
}

// ---------- actions ----------
async function reveal(path: string) {
  try { await api.reveal(path); }
  catch (e) { notify(`Could not open Explorer: ${String(e)}`); }
}

async function copyPath(path: string) {
  try {
    await navigator.clipboard.writeText(path);
    notify("Path copied");
  } catch {
    notify("Could not copy the path");
  }
}

function notify(text: string) {
  const el = document.createElement("div");
  el.className = "toast toast-note";
  el.setAttribute("role", "status");
  el.textContent = text;
  $("#toasts").append(el);
  setTimeout(() => { el.classList.add("leaving"); setTimeout(() => el.remove(), 170); }, 2600);
}

type Target = { path: string; size: number; name: string };

function confirmRecycle(items: Target[]) {
  closeMenu();
  const total = items.reduce((s, x) => s + x.size, 0);
  const m = $("#modal");
  m.hidden = false;
  m.innerHTML = `<div class="modal" role="alertdialog" aria-modal="true" aria-labelledby="mt">
    <div class="modal-icon">${icon("trash")}</div>
    <h2 id="mt">Move ${items.length === 1 ? esc(items[0].name) : `${items.length} items`} to the Recycle Bin?</h2>
    <p>${size(total)} will be freed once you empty the Recycle Bin. Until then you can restore it.</p>
    <ul class="modal-paths">${items.map((x) => `<li><span>${pathHtml(x.path)}</span><span class="num">${size(x.size)}</span></li>`).join("")}</ul>
    <div class="row-btns"><button class="btn ghost" data-m="no">Cancel</button><button class="btn danger" data-m="yes">Move to Recycle Bin</button></div>
  </div>`;
  const yes = m.querySelector<HTMLButtonElement>("[data-m='yes']")!;
  const close = () => { m.hidden = true; m.innerHTML = ""; };
  m.querySelector("[data-m='no']")!.addEventListener("click", close);
  m.addEventListener("mousedown", (e) => { if (e.target === m) close(); }, { once: true });
  yes.focus();
  yes.addEventListener("click", async () => {
    yes.disabled = true;
    yes.textContent = "Moving…";
    try {
      await api.recycle(items.map((x) => x.path));
      close();
      notify(`Moved to the Recycle Bin · ${size(total)}`);
      S.selected = null;
      if (S.target) scan(S.target, { background: true });
    } catch (e) {
      yes.disabled = false;
      yes.textContent = "Move to Recycle Bin";
      const p = m.querySelector("p")!;
      p.className = "modal-error";
      p.textContent = String(e);
    }
  });
}

// ---------- context menu ----------
function menuFor(it: Item, x: number, y: number) {
  const m = $("#menu");
  const isRoot = it.id === 0;
  const items: [string, string, string, boolean?][] = [
    ...(it.is_dir ? [["open", "folder", "Open in map"] as [string, string, string]] : [["locate", "folder", "Show in map"] as [string, string, string]]),
    ["reveal", "reveal", "Show in Explorer"],
    ["copy", "copy", "Copy path"],
    ...(isRoot ? [] : [["trash", "trash", "Move to Recycle Bin…", true] as [string, string, string, boolean]]),
  ];
  m.innerHTML = `<div class="menu-head"><b>${esc(it.name || it.path)}</b><span class="num">${size(it.size)}</span></div>` +
    items.map(([a, ic, label, danger]) => `<button role="menuitem" class="${danger ? "danger-text" : ""}" data-menu="${a}">${icon(ic)}${label}</button>`).join("");
  m.hidden = false;
  const r = m.getBoundingClientRect();
  m.style.left = `${Math.min(x, window.innerWidth - r.width - 8)}px`;
  m.style.top = `${Math.min(y, window.innerHeight - r.height - 8)}px`;
  m.querySelectorAll<HTMLButtonElement>("[data-menu]").forEach((b) =>
    b.addEventListener("click", () => {
      closeMenu();
      const a = b.dataset.menu;
      if (a === "open") open(it.id);
      if (a === "locate") revealInMap(it.path);
      if (a === "reveal") reveal(it.path);
      if (a === "copy") copyPath(it.path);
      if (a === "trash") confirmRecycle([{ path: it.path, size: it.size, name: it.name }]);
    }),
  );
  m.querySelector<HTMLButtonElement>("button")?.focus();
}

function closeMenu() {
  const m = document.getElementById("menu");
  if (m && !m.hidden) { m.hidden = true; m.innerHTML = ""; }
}

// ---------- lower panel ----------
function setTab(t: Tab) {
  if (t !== S.tab) S.typeExt = null;
  S.tab = t;
  const btns = [...document.querySelectorAll<HTMLButtonElement>("[data-tab]")];
  btns.forEach((b) => b.classList.toggle("on", b.dataset.tab === t));
  const on = btns.find((b) => b.dataset.tab === t);
  const knob = $(".seg .knob");
  if (on) {
    knob.style.opacity = "1";
    knob.style.width = `${on.offsetWidth}px`;
    knob.style.transform = `translateX(${on.offsetLeft - 2}px)`;
  } else knob.style.opacity = "0";
  panel();
}

const row = (o: { path: string; name: string; dir: boolean; right: string; bar?: number; barColor?: string; extra?: string; up?: boolean; size?: number; trash?: boolean }) => `
  <div class="row" data-path="${esc(o.path)}" data-dir="${o.dir ? 1 : 0}" data-size="${o.size ?? 0}" data-name="${esc(o.name)}">
    ${mark(o.name, o.dir)}
    <div class="name"><b>${esc(o.name)}${o.extra ?? ""}</b><span class="path">${esc(o.path)}</span></div>
    <div class="bar">${o.bar != null ? `<i style="width:${Math.max(1.5, Math.min(100, o.bar * 100)).toFixed(1)}%;background:${o.barColor ?? "var(--ink-3)"}"></i>` : ""}</div>
    <div class="size ${o.up ? "up" : ""}">${o.right}${o.trash ? `<button class="row-trash" data-rowact="trash" title="Move to Recycle Bin" aria-label="Move to Recycle Bin">${icon("trash")}</button>` : ""}</div>
  </div>`;
const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

function chips<T extends string | number>(opts: [T, string][], cur: T, key: string) {
  return `<div class="chips" role="radiogroup">${opts.map(([v, l]) => `<button role="radio" aria-checked="${v === cur}" class="${v === cur ? "on" : ""}" data-${key}="${v}">${l}</button>`).join("")}</div>`;
}

let panelSeq = 0;
async function panel() {
  if (!S.info) return;
  const seq = ++panelSeq;
  const node = S.node;
  const list = $("#list");
  const note = $("#note");
  const tools = $("#tools");
  const scope = S.folder?.size || 1;
  const where = S.folder?.id === 0 ? "this drive" : S.folder?.name || "this folder";
  let html = "";
  let msg = "";
  let tl = "";
  if (S.tab === "largest") {
    const [files, hot] = await Promise.all([api.topFiles(node, 40), api.hotspots(node, 12)]);
    html = `<div class="group-head">Heaviest folders</div>` +
      hot.map((h) => row({ path: h.path, name: h.name, dir: true, right: size(h.size), bar: h.size / scope, barColor: "var(--accent)", size: h.size, trash: true })).join("") +
      `<div class="group-head">Largest files</div>` +
      files.map((f) => row({ path: f.path, name: f.name, dir: false, right: size(f.size), bar: f.size / scope, barColor: rowIcon(f), size: f.size, trash: true })).join("");
    msg = `in ${where}`;
  } else if (S.tab === "types" && S.typeExt !== null) {
    const ext = S.typeExt;
    const k = kindOf(ext);
    const rows = await api.extFolders(node, ext, 2000);
    const total = rows.reduce((s, r) => s + r.size, 0);
    const mx = Math.max(1, ...rows.map((r) => r.size));
    msg = `${ext ? "." + ext : "No extension"} in ${rows.length.toLocaleString()}${rows.length >= 2000 ? "+" : ""} folders`;
    html = `<div class="row type-back" data-typeback><span class="fold">${icon("back")}</span>
        <div class="name"><b>All types</b><span class="path" style="direction:ltr">${ext ? "." + esc(ext) : "No extension"} · ${size(total)} in the folders below</span></div><div></div><div></div></div>` +
      (rows.length
        ? rows.map((r) => row({ path: r.item.path, name: r.item.name || r.item.path, dir: true, right: size(r.size), bar: r.size / mx, barColor: color(k), extra: `<span class="tag system">${r.files.toLocaleString()} files</span>` })).join("")
        : `<div class="empty"><b>Nothing here</b>No files of this type in ${esc(where)}.</div>`);
  } else if (S.tab === "types") {
    const types = await api.types(node, 40);
    html = types.map((t) => {
      const k = kindOf(t.ext);
      return `<div class="row type-row" data-ext="${esc(t.ext)}" title="Show the folders where these files live"><span class="swatch" style="background:${color(k)}"></span>
        <div class="name"><b>${t.ext ? "." + esc(t.ext) : "No extension"}</b><span class="path" style="direction:ltr">${LABEL[k]} · ${t.files.toLocaleString()} files</span></div>
        <div class="bar"><i style="width:${Math.max(1.5, (t.size * 100) / scope).toFixed(1)}%;background:${color(k)}"></i></div>
        <div class="size">${size(t.size)}</div></div>`;
    }).join("");
    msg = `by extension · click one to see its folders`;
  } else if (S.tab === "growth") {
    tl = chips<Since>([["last", "Last scan"], ["day", "Day"], ["week", "Week"], ["month", "Month"]], S.since, "since");
    list.innerHTML = `<div class="empty"><b>Comparing scans…</b></div>`;
    const g = await api.growth(S.since);
    if (!g) html = `<div class="empty"><b>No earlier scan to compare</b>Every scan is saved. Next time Gigamap shows what grew in between.</div>`;
    else {
      const since = new Date(g.since * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
      msg = `since ${since}: ${g.total_delta >= 0 ? "+" : "−"}${size(Math.abs(g.total_delta))}`;
      const mx = Math.max(1, ...g.grown.map((c) => c.delta), ...g.new_files.map((c) => c.after));
      html = (g.grown.length ? `<div class="group-head">Grew</div>` + g.grown.map((c) => row({ path: c.path, name: baseName(c.path), dir: true, right: "+" + size(c.delta), bar: c.delta / mx, barColor: "#f0645a", up: true, extra: c.is_new ? `<span class="tag review">new</span>` : "" })).join("") : "") +
        (g.new_files.length ? `<div class="group-head">New or changed big files</div>` + g.new_files.map((c) => row({ path: c.path, name: baseName(c.path), dir: false, right: size(c.after), bar: c.after / mx, barColor: rowIcon({ name: c.path, is_dir: false }), size: c.after, trash: true })).join("") : "") +
        (g.shrunk.length ? `<div class="group-head">Shrank</div>` + g.shrunk.map((c) => row({ path: c.path, name: baseName(c.path), dir: true, right: "−" + size(-c.delta) })).join("") : "") ||
        `<div class="empty"><b>Nothing changed much</b>No folder moved by more than a megabyte.</div>`;
    }
  } else if (S.tab === "cleanup") {
    const c = await api.cleanup(node);
    const safe = c.filter((x) => x.safety === "Safe").reduce((s, x) => s + x.size, 0);
    msg = `${size(safe)} regenerates on its own`;
    const mx = Math.max(1, ...c.map((x) => x.size));
    html = c.length
      ? c.map((x) => row({ path: x.path, name: x.kind, dir: x.is_dir, right: size(x.size), bar: x.size / mx, barColor: x.safety === "Safe" ? "#33b884" : x.safety === "Review" ? "#f6a33b" : "var(--ink-3)", size: x.size, trash: x.safety !== "System", extra: `<span class="tag ${x.safety.toLowerCase()}" title="${esc(x.why)}">${x.safety === "Safe" ? "safe" : x.safety === "Review" ? "review" : "Windows"}</span>` })).join("")
      : `<div class="empty"><b>Nothing to clean here</b>No build output, caches or temp folders over 16 MB in ${esc(where)}.</div>`;
  } else if (S.tab === "dupes") {
    tl = chips<number>([[10, "≥ 10 MB"], [50, "≥ 50 MB"], [200, "≥ 200 MB"], [1024, "≥ 1 GB"]], S.dupesMin, "min");
    if (!S.dupesRun) {
      html = `<div class="empty"><b>Find identical files in ${esc(where)}</b>Only files of the same size are read and compared, biggest first. On a large drive this reads a lot from disk.
        <div style="margin-top:14px"><button class="btn primary" id="dupes-go">${icon("copies")}Find duplicates</button></div></div>`;
      msg = "";
    } else {
      list.innerHTML = `<div class="empty"><b>Comparing files…</b>Same-size files are hashed, biggest first.</div>`;
      const groups = await api.duplicates(node, S.dupesMin);
      const wasted = groups.reduce((s, g) => s + g.wasted, 0);
      msg = groups.length ? `${size(wasted)} in extra copies` : "";
      html = groups.length
        ? groups.map((g) => `<div class="group-head num">${g.paths.length} copies · ${size(g.size)} each</div>` + g.paths.map((p) => row({ path: p, name: baseName(p), dir: false, right: size(g.size), size: g.size, trash: true })).join("")).join("")
        : `<div class="empty"><b>No duplicates</b>No identical files of ${S.dupesMin >= 1024 ? "1 GB" : S.dupesMin + " MB"} or more in ${esc(where)}.</div>`;
    }
  } else if (S.tab === "search") {
    const at = S.queryWhole ? 0 : node;
    const hits = await api.search(at, S.query, 300);
    msg = `${hits.length >= 300 ? "300+" : hits.length} matches for “${S.query}” in ${S.queryWhole || node === 0 ? "the whole drive" : where}`;
    if (!S.queryWhole && node !== 0) tl = `<button class="btn ghost small" id="search-all">${icon("search")}Search the whole drive</button>`;
    html = hits.length ? hits.map((h) => row({ path: h.path, name: h.name, dir: h.is_dir, right: size(h.size), bar: h.size / scope, barColor: rowIcon(h), size: h.size, trash: true })).join("")
      : `<div class="empty"><b>No matches</b>Try part of a name, or a pattern like *.mp4</div>`;
  }
  if (seq !== panelSeq) return;
  note.textContent = msg;
  tools.innerHTML = tl;
  list.innerHTML = html;
  list.scrollTop = 0;
  bindRows(list);
  list.querySelectorAll<HTMLElement>("[data-ext]").forEach((r) =>
    r.addEventListener("click", () => { S.typeExt = r.dataset.ext ?? ""; panel(); }),
  );
  list.querySelector<HTMLElement>("[data-typeback]")?.addEventListener("click", () => { S.typeExt = null; panel(); });
  tools.querySelectorAll<HTMLElement>("[data-since]").forEach((b) =>
    b.addEventListener("click", () => { S.since = b.dataset.since as Since; panel(); }),
  );
  tools.querySelectorAll<HTMLElement>("[data-min]").forEach((b) =>
    b.addEventListener("click", () => { S.dupesMin = Number(b.dataset.min); panel(); }),
  );
  document.getElementById("dupes-go")?.addEventListener("click", () => { S.dupesRun = true; panel(); });
  document.getElementById("search-all")?.addEventListener("click", () => { S.queryWhole = true; panel(); });
}

// ---------- agent delete requests ----------
const shown = new Set<string>();
async function pollRequests() {
  try {
    const reqs = await api.pending();
    for (const r of reqs) if (!shown.has(r.id)) toast(r);
  } catch { /* app may be mid-scan */ }
  setTimeout(pollRequests, 2000);
}

function toast(r: DeleteRequest) {
  shown.add(r.id);
  const total = r.sizes.reduce((s, x) => s + x, 0);
  const el = document.createElement("div");
  el.className = "toast";
  el.setAttribute("role", "alertdialog");
  el.innerHTML = `
    <div class="toast-head"><span class="badge">${icon("bot")}</span>
      <div><b>An agent wants to free ${size(total)}</b><span>${esc(r.reason || "No reason given.")}</span></div></div>
    <ul>${r.paths.map((p, k) => `<li data-path="${esc(p)}" title="Show in map"><span>${pathHtml(p)}</span><span class="num">${size(r.sizes[k] || 0)}</span></li>`).join("")}</ul>
    <div class="toast-msg" hidden></div>
    <div class="row-btns"><button class="btn ghost" data-a="no">Decline</button><button class="btn danger" data-a="yes">Move to Recycle Bin</button></div>`;
  $("#toasts").append(el);
  el.querySelectorAll<HTMLElement>("li[data-path]").forEach((li) => li.addEventListener("click", () => revealInMap(li.dataset.path!)));
  el.querySelectorAll<HTMLButtonElement>("[data-a]").forEach((b) =>
    b.addEventListener("click", async () => {
      const yes = b.dataset.a === "yes";
      el.querySelectorAll("button").forEach((x) => ((x as HTMLButtonElement).disabled = true));
      try {
        const res = await api.resolve(r.id, yes);
        if (yes && res.state !== "done") {
          const m = el.querySelector<HTMLElement>(".toast-msg")!;
          m.hidden = false;
          m.textContent = res.state === "expired" ? "This request expired, nothing was moved." : `Nothing was moved: ${res.message || res.state}`;
          setTimeout(() => { el.classList.add("leaving"); setTimeout(() => el.remove(), 170); }, 4000);
          return;
        }
      } catch (e) {
        notify(`Could not answer the request: ${String(e)}`);
      }
      el.classList.add("leaving");
      setTimeout(() => el.remove(), 170);
      if (yes) {
        notify(`Moved to the Recycle Bin · ${size(total)}`);
        if (S.target) scan(S.target, { background: true });
      }
    }),
  );
}

// ---------- keyboard ----------
function keys(e: KeyboardEvent) {
  const typing = (e.target as HTMLElement).tagName === "INPUT";
  if (e.key === "Escape") {
    if (!$("#modal").hidden) { $("#modal").hidden = true; $("#modal").innerHTML = ""; return; }
    if (!$("#menu").hidden) { closeMenu(); return; }
    if (typing) {
      const q = $<HTMLInputElement>("#q");
      q.value = "";
      q.blur();
      S.query = "";
      if (S.tab === "search") setTab("largest");
      return;
    }
    if (S.selected) { S.selected = null; map.select(-1); inspector(); return; }
  }
  if (e.ctrlKey && e.key.toLowerCase() === "f") {
    e.preventDefault();
    $<HTMLInputElement>("#q").focus();
    return;
  }
  if (typing || !$("#modal").hidden) return;
  if (e.key === "F5") { e.preventDefault(); if (S.target) scan(S.target, { background: true }); }
  if (e.key === "Backspace" || (e.altKey && e.key === "ArrowLeft")) { e.preventDefault(); up(); }
  if (e.key === "Enter" && S.selected?.is_dir) open(S.selected.id);
  if (e.key === "Delete" && S.selected && S.selected.id !== 0) confirmRecycle([{ path: S.selected.path, size: S.selected.size, name: S.selected.name }]);
}

shell();
syncUp();
setupMap();
setTab("largest");
loadDrives();
pollRequests();
