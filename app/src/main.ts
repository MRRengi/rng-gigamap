import { api, inTauri, windowAction, type Drive, type Item, type ScanInfo, type Slice, type DeleteRequest } from "./api";
import { Treemap } from "./treemap";
import { color, esc, icon, kindOf, KIND_ICON, LABEL, LEGEND, pct, size } from "./kinds";

type Tab = "largest" | "types" | "growth" | "cleanup" | "dupes" | "search";

const S = {
  drives: [] as Drive[],
  info: null as ScanInfo | null,
  target: "",
  node: 0,
  slice: null as Slice | null,
  selected: null as Item | null,
  folder: null as Item | null,
  tab: "largest" as Tab,
  scanning: false,
  confirm: false,
  query: "",
};

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector(sel) as T;

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
      </div>
      <div id="mode" class="side-foot"></div>
    </aside>
    <main class="main">
      <header class="titlebar" data-drag>
        <nav class="crumbs" id="crumbs" aria-label="Location"></nav>
        <div class="tool-group">
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
  ${inTauri ? "" : `<div class="preview-badge">Browser preview with a synthetic drive</div>`}`;

  document.querySelectorAll<HTMLElement>("[data-drag]").forEach((el) =>
    el.addEventListener("mousedown", (e) => {
      if ((e.target as HTMLElement).closest("button,input,label")) return;
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
  $("#rescan").addEventListener("click", () => S.target && scan(S.target, true));
  let t = 0;
  $<HTMLInputElement>("#q").addEventListener("input", (e) => {
    clearTimeout(t);
    const v = (e.target as HTMLInputElement).value.trim();
    t = window.setTimeout(() => {
      S.query = v;
      setTab(v ? "search" : S.tab === "search" ? "largest" : S.tab);
    }, 180);
  });
  window.addEventListener("keydown", keys);
  // Mouse back button goes up a level, like Explorer.
  window.addEventListener("mouseup", (e) => {
    if (e.button === 3) { e.preventDefault(); up(); }
  });
}

// ---------- treemap ----------
let map: Treemap;
function setupMap() {
  map = new Treemap($("#map"));
  const tip = $("#tip");
  map.onHover = (i, x, y) => {
    if (i < 0 || !S.slice) return tip.classList.remove("show");
    const s = S.slice;
    const parentSize = s.size[0] || 1;
    tip.innerHTML = `<div class="t-name">${esc(s.name[i])}${s.dir[i] ? "\\" : ""}</div><div class="t-sub num">${size(s.size[i])} · ${pct(s.size[i], parentSize)} of this view</div>`;
    const box = $("#map").getBoundingClientRect();
    const tw = tip.offsetWidth, th = tip.offsetHeight;
    tip.style.left = `${Math.min(x + 14, box.width - tw - 8)}px`;
    tip.style.top = `${y + th + 22 > box.height ? y - th - 10 : y + 18}px`;
    tip.classList.add("show");
  };
  map.onSelect = async (i) => {
    if (!S.slice) return;
    map.select(i);
    S.confirm = false;
    S.selected = await api.item(S.slice.id[i]);
    inspector();
  };
  map.onOpen = (i) => S.slice && open(S.slice.id[i], map.rectOf(i));
}

// ---------- data flow ----------
async function loadDrives() {
  S.drives = await api.drives();
  drives();
  if (!S.info) picker();
}

async function scan(target: string, keepPlace = false) {
  const where = keepPlace && S.folder ? S.folder.path : null;
  S.target = target;
  S.scanning = true;
  drives();
  $("#mapstate").innerHTML = `<div class="map-status"><div><div class="big">Mapping ${esc(target.replace(/\\$/, ""))}</div><div>Reading the file table</div><div class="scanning-bar"><i></i></div></div></div>`;
  $("#legend").style.opacity = "0";
  try {
    S.info = await api.scan(target);
  } catch (e) {
    $("#mapstate").innerHTML = `<div class="map-status"><div><div class="big">Could not read ${esc(target)}</div><div>${esc(String(e))}</div></div></div>`;
    S.scanning = false;
    drives();
    return;
  }
  S.scanning = false;
  $("#mapstate").innerHTML = "";
  $("#legend").style.opacity = "1";
  drives();
  mode();
  const back = where ? await api.find(where) : null;
  await open(back ?? 0);
}

function syncUp() {
  const b = document.querySelector<HTMLButtonElement>("#up");
  if (b) b.disabled = !S.info || S.node === 0;
}

async function open(node: number, from?: { x: number; y: number; w: number; h: number }, reverse = false) {
  const [slice, trail] = await Promise.all([api.tree(node, 24000), api.ancestors(node)]);
  if (from) await map.zoomFrom(from, reverse);
  S.node = node;
  S.slice = slice;
  S.folder = trail[trail.length - 1];
  S.selected = null;
  S.confirm = false;
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
  S.confirm = false;
  const local = S.slice ? S.slice.id.indexOf(id) : -1;
  map.select(local);
  inspector();
}

// ---------- views ----------
function drives() {
  $("#drives").innerHTML = S.drives
    .map((d) => {
      const used = d.total ? (d.total - d.free) / d.total : 0;
      const on = S.target === d.root;
      const busy = on && S.scanning;
      return `<button class="side-row ${on ? "on" : ""}" data-root="${esc(d.root)}">
        ${icon("drive")}
        <div class="drive-meta">
          <div class="drive-name">${esc(d.label || "Local Disk")} (${esc(d.root.replace(/\\$/, ""))})</div>
          <div class="drive-bar"><i class="${used > 0.95 ? "full" : used > 0.85 ? "high" : ""}" style="width:${(used * 100).toFixed(1)}%"></i></div>
          <div class="drive-sub num">${busy ? "Scanning…" : d.total ? `${size(d.free)} free of ${size(d.total)}` : esc(d.fs)}</div>
        </div></button>`;
    })
    .join("");
  document.querySelectorAll<HTMLButtonElement>("#drives [data-root]").forEach((b) =>
    b.addEventListener("click", () => scan(b.dataset.root!)),
  );
}

function mode() {
  const el = $("#mode");
  const i = S.info;
  if (!i) return (el.innerHTML = "");
  const fast = i.method === "Mft";
  el.className = `side-foot ${fast ? "fast" : ""}`;
  el.innerHTML = fast
    ? `${icon("bolt")}<div><b>Fast mode</b>${i.items.toLocaleString()} items in ${(i.scan_ms / 1000).toFixed(1)} s, read from the NTFS file table${i.via_service ? "" : " (elevated)"}.</div>`
    : `${icon("turtle")}<div><b>Standard mode</b>Folders were walked one by one. Install the Gigamap service for scans in seconds.</div>`;
}

function picker() {
  $("#crumbs").innerHTML = "";
  $("#legend").style.opacity = "0";
  $("#mapstate").innerHTML = `<div class="picker">
    <h1>What is filling your disk?</h1>
    <p>Pick a drive. NTFS drives are mapped straight from the file table, usually in seconds.</p>
    <div class="picker-list">${S.drives
      .map((d) => {
        const used = d.total ? (d.total - d.free) / d.total : 0;
        return `<button class="picker-row" data-root="${esc(d.root)}">${icon("drive")}
        <div class="drive-meta"><div class="drive-name">${esc(d.label || "Local Disk")} (${esc(d.root.replace(/\\$/, ""))})</div>
        <div class="drive-bar"><i class="${used > 0.95 ? "full" : used > 0.85 ? "high" : ""}" style="width:${(used * 100).toFixed(1)}%"></i></div>
        <div class="drive-sub num">${size(d.total - d.free)} used · ${size(d.free)} free · ${esc(d.fs)}</div></div>
        <span class="go">${icon("chevron")}</span></button>`;
      })
      .join("")}</div></div>`;
  document.querySelectorAll<HTMLButtonElement>(".picker [data-root]").forEach((b) =>
    b.addEventListener("click", () => scan(b.dataset.root!)),
  );
  $("#insp").innerHTML = `<div class="empty" style="margin-top:40px"><b>Nothing selected</b>Choose a drive to see what takes the space.</div>`;
  $("#list").innerHTML = "";
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

async function inspector() {
  const it = S.selected ?? S.folder;
  if (!it || !S.info) return;
  const kind = kindOf(it.name.split(".").pop()!.toLowerCase(), it.is_dir);
  const parentTotal = S.folder && S.selected ? S.folder.size : S.info.total;
  const shareOf = S.selected ? (S.folder?.name || "this drive") : "drive";
  const shareTotal = S.selected ? parentTotal : (S.info.volume_total || S.info.total);
  const isRoot = it.id === 0;
  const kids = it.is_dir ? await api.children(it.id, 40) : [];
  const date = it.mtime ? new Date(it.mtime * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }) : "—";
  $("#insp").innerHTML = `
    <div class="insp-head">
      <div class="insp-icon" style="background:${it.is_dir ? "var(--accent)" : color(kind)}">${icon(KIND_ICON[kind])}</div>
      <div class="insp-title"><b>${esc(isRoot ? it.path : it.name)}</b><span>${it.is_dir ? "Folder" : LABEL[kind]}</span></div>
    </div>
    <div>
      <div class="insp-size num">${size(it.size)}<small>${pct(it.size, shareTotal)} of ${esc(isRoot ? "the volume" : shareOf)}</small></div>
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
      ${isRoot ? "" : S.confirm
        ? `<div class="confirm"><p>Move <b>${esc(it.name)}</b> (${size(it.size)}) to the Recycle Bin? You can restore it from there.</p>
           <div class="row-btns"><button class="btn ghost" data-act="cancel">Cancel</button><button class="btn danger" data-act="recycle">Move to Recycle Bin</button></div></div>`
        : `<button class="btn ghost" data-act="ask">${icon("trash")}Move to Recycle Bin…</button>`}
    </div>
    ${kids.length ? `<div class="insp-list-title">Inside</div><div class="insp-list">${kids
      .map((k) => `<div class="row" data-path="${esc(k.path)}" data-dir="${k.is_dir ? 1 : 0}" data-id="${k.id}">
        ${mark(k.name, k.is_dir)}
        <div class="name"><b>${esc(k.name)}</b></div>
        <div class="size">${size(k.size)}</div></div>`)
      .join("")}</div>` : ""}`;
  $("#insp").querySelectorAll<HTMLButtonElement>("[data-act]").forEach((b) =>
    b.addEventListener("click", async () => {
      const a = b.dataset.act;
      if (a === "open" && S.selected) open(S.selected.id, S.slice ? map.rectOf(S.slice.id.indexOf(S.selected.id)) : undefined);
      if (a === "reveal") api.reveal(it.path);
      if (a === "copy") {
        await navigator.clipboard?.writeText(it.path);
        b.innerHTML = `${icon("copy")}Copied`;
        setTimeout(() => (b.innerHTML = `${icon("copy")}Copy path`), 1200);
      }
      if (a === "ask") { S.confirm = true; inspector(); }
      if (a === "cancel") { S.confirm = false; inspector(); }
      if (a === "recycle") {
        b.disabled = true;
        try {
          await api.recycle([it.path]);
          S.confirm = false;
          await scan(S.target, true);
        } catch (e) {
          b.disabled = false;
          b.textContent = String(e);
        }
      }
    }),
  );
  bindRows($("#insp"));
}

function bindRows(scope: HTMLElement) {
  scope.querySelectorAll<HTMLElement>(".row[data-path]").forEach((r) => {
    r.addEventListener("click", () => {
      scope.querySelectorAll(".row.sel").forEach((x) => x.classList.remove("sel"));
      r.classList.add("sel");
      focusPath(r.dataset.path!);
    });
    r.addEventListener("dblclick", async () => {
      const id = await api.find(r.dataset.path!);
      if (id == null) return;
      if (r.dataset.dir === "1") return open(id);
      const trail = await api.ancestors(id);
      await open(trail[trail.length - 2].id);
      focusPath(r.dataset.path!);
    });
  });
}

// ---------- lower panel ----------
function setTab(t: Tab) {
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

const row = (o: { path: string; name: string; dir: boolean; right: string; bar?: number; barColor?: string; extra?: string; up?: boolean }) => `
  <div class="row" data-path="${esc(o.path)}" data-dir="${o.dir ? 1 : 0}">
    ${mark(o.name, o.dir)}
    <div class="name"><b>${esc(o.name)}${o.extra ?? ""}</b><span class="path">${esc(o.path)}</span></div>
    <div class="bar">${o.bar != null ? `<i style="width:${Math.max(1.5, Math.min(100, o.bar * 100)).toFixed(1)}%;background:${o.barColor ?? "var(--ink-3)"}"></i>` : ""}</div>
    <div class="size ${o.up ? "up" : ""}">${o.right}</div>
  </div>`;
const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

let panelSeq = 0;
async function panel() {
  if (!S.info) return;
  const seq = ++panelSeq;
  const node = S.node;
  const list = $("#list");
  const note = $("#note");
  const scope = S.folder?.size || 1;
  let html = "";
  let msg = "";
  if (S.tab === "largest") {
    const [files, hot] = await Promise.all([api.topFiles(node, 40), api.hotspots(node, 12)]);
    html = `<div class="group-head">Heaviest folders</div>` +
      hot.map((h) => row({ path: h.path, name: h.name, dir: true, right: size(h.size), bar: h.size / scope, barColor: "var(--accent)" })).join("") +
      `<div class="group-head">Largest files</div>` +
      files.map((f) => row({ path: f.path, name: f.name, dir: false, right: size(f.size), bar: f.size / scope, barColor: rowIcon(f) })).join("");
    msg = `in ${S.folder?.name || S.folder?.path}`;
  } else if (S.tab === "types") {
    const types = await api.types(node, 40);
    html = types.map((t) => {
      const k = kindOf(t.ext);
      return `<div class="row"><span class="swatch" style="background:${color(k)}"></span>
        <div class="name"><b>${t.ext ? "." + esc(t.ext) : "No extension"}</b><span class="path" style="direction:ltr">${LABEL[k]} · ${t.files.toLocaleString()} files</span></div>
        <div class="bar"><i style="width:${Math.max(1.5, (t.size * 100) / scope).toFixed(1)}%;background:${color(k)}"></i></div>
        <div class="size">${size(t.size)}</div></div>`;
    }).join("");
    msg = "by extension";
  } else if (S.tab === "growth") {
    const g = await api.growth();
    if (!g) html = `<div class="empty"><b>No earlier scan to compare</b>This scan is saved. Next time Gigamap shows what grew in between.</div>`;
    else {
      const since = new Date(g.since * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
      msg = `since ${since}: ${g.total_delta >= 0 ? "+" : "−"}${size(Math.abs(g.total_delta))}`;
      const mx = Math.max(1, ...g.grown.map((c) => c.delta), ...g.new_files.map((c) => c.after));
      html = (g.grown.length ? `<div class="group-head">Grew</div>` + g.grown.map((c) => row({ path: c.path, name: baseName(c.path), dir: true, right: "+" + size(c.delta), bar: c.delta / mx, barColor: "#f0645a", up: true, extra: c.is_new ? `<span class="tag review">new</span>` : "" })).join("") : "") +
        (g.new_files.length ? `<div class="group-head">New or changed big files</div>` + g.new_files.map((c) => row({ path: c.path, name: baseName(c.path), dir: false, right: size(c.after), bar: c.after / mx, barColor: rowIcon({ name: c.path, is_dir: false }) })).join("") : "") +
        (g.shrunk.length ? `<div class="group-head">Shrank</div>` + g.shrunk.map((c) => row({ path: c.path, name: baseName(c.path), dir: true, right: "−" + size(-c.delta) })).join("") : "") ||
        `<div class="empty"><b>Nothing changed much</b>No folder moved by more than a megabyte.</div>`;
    }
  } else if (S.tab === "cleanup") {
    const c = await api.cleanup(node);
    const safe = c.filter((x) => x.safety === "Safe").reduce((s, x) => s + x.size, 0);
    msg = `${size(safe)} regenerates on its own`;
    const mx = Math.max(1, ...c.map((x) => x.size));
    html = c.length
      ? c.map((x) => row({ path: x.path, name: x.kind, dir: true, right: size(x.size), bar: x.size / mx, barColor: x.safety === "Safe" ? "#33b884" : x.safety === "Review" ? "#f6a33b" : "var(--ink-3)", extra: `<span class="tag ${x.safety.toLowerCase()}">${x.safety === "Safe" ? "safe" : x.safety === "Review" ? "review" : "Windows"}</span>` })).join("")
      : `<div class="empty"><b>Nothing to clean here</b>No build output, caches or temp folders over 16 MB.</div>`;
  } else if (S.tab === "dupes") {
    list.innerHTML = `<div class="empty"><b>Comparing files…</b>Only same-size files are read, biggest first.</div>`;
    const groups = await api.duplicates(node, 50);
    const wasted = groups.reduce((s, g) => s + g.wasted, 0);
    msg = groups.length ? `${size(wasted)} in extra copies` : "files of 50 MB and up";
    html = groups.length
      ? groups.map((g) => `<div class="group-head num">${g.paths.length} copies · ${size(g.size)} each</div>` + g.paths.map((p) => row({ path: p, name: baseName(p), dir: false, right: size(g.size) })).join("")).join("")
      : `<div class="empty"><b>No duplicates</b>No identical files of 50 MB or more in this folder.</div>`;
  } else if (S.tab === "search") {
    const hits = await api.search(node, S.query, 200);
    msg = `${hits.length} matches for “${S.query}”`;
    html = hits.length ? hits.map((h) => row({ path: h.path, name: h.name, dir: h.is_dir, right: size(h.size), bar: h.size / scope, barColor: rowIcon(h) })).join("")
      : `<div class="empty"><b>No matches</b>Try part of a name, or a pattern like *.mp4</div>`;
  }
  if (seq !== panelSeq) return;
  note.textContent = msg;
  list.innerHTML = html;
  list.scrollTop = 0;
  bindRows(list);
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
    <ul>${r.paths.map((p, k) => `<li><span>${esc(p)}</span><span class="num">${size(r.sizes[k] || 0)}</span></li>`).join("")}</ul>
    <div class="row-btns"><button class="btn ghost" data-a="no">Decline</button><button class="btn danger" data-a="yes">Move to Recycle Bin</button></div>`;
  $("#toasts").append(el);
  el.querySelectorAll<HTMLButtonElement>("[data-a]").forEach((b) =>
    b.addEventListener("click", async () => {
      const yes = b.dataset.a === "yes";
      el.querySelectorAll("button").forEach((x) => ((x as HTMLButtonElement).disabled = true));
      await api.resolve(r.id, yes);
      el.classList.add("leaving");
      setTimeout(() => el.remove(), 170);
      if (yes && S.target) scan(S.target, true);
    }),
  );
}

// ---------- keyboard ----------
function keys(e: KeyboardEvent) {
  const typing = (e.target as HTMLElement).tagName === "INPUT";
  if (e.ctrlKey && e.key.toLowerCase() === "f") {
    e.preventDefault();
    $<HTMLInputElement>("#q").focus();
    return;
  }
  if (e.key === "Escape" && typing) {
    const q = $<HTMLInputElement>("#q");
    q.value = "";
    q.blur();
    S.query = "";
    if (S.tab === "search") setTab("largest");
    return;
  }
  if (typing) return;
  if (e.key === "F5") { e.preventDefault(); S.target && scan(S.target, true); }
  if (e.key === "Backspace" || (e.altKey && e.key === "ArrowLeft")) { e.preventDefault(); up(); }
  if (e.key === "Enter" && S.selected?.is_dir) open(S.selected.id);
  if (e.key === "Delete" && S.selected && S.selected.id !== 0) { S.confirm = true; inspector(); }
}

shell();
syncUp();
setupMap();
setTab("largest");
loadDrives();
pollRequests();
