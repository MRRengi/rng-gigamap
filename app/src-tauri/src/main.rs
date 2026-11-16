#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gigamap_core::cleanup::{self, Candidate};
use gigamap_core::diff::{self, Growth};
use gigamap_core::dupes::{self, DupeGroup};
use gigamap_core::query::{self, ExtFolder, Item, Slice, TypeStat};
use gigamap_core::requests::{self, DeleteRequest};
use gigamap_core::{ipc, mft, snapshot, walk, Index, NodeId};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Only the current index lives in memory; older scans stay on disk until Growth asks.
#[derive(Default)]
struct AppState {
    idx: RwLock<Option<Arc<Index>>>,
    scanning: AtomicBool,
}

type St<'a> = tauri::State<'a, AppState>;
type Res<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

fn current(st: &St) -> Res<Arc<Index>> {
    st.idx.read().unwrap().clone().ok_or_else(|| "nothing scanned yet".to_string())
}

#[derive(Serialize)]
struct Drive {
    root: String,
    label: String,
    total: u64,
    free: u64,
    fs: String,
}

#[cfg(windows)]
fn drives() -> Vec<Drive> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLogicalDrives() -> u32;
        fn GetDriveTypeW(root: *const u16) -> u32;
        fn GetVolumeInformationW(
            root: *const u16,
            name: *mut u16,
            name_len: u32,
            serial: *mut u32,
            max_comp: *mut u32,
            flags: *mut u32,
            fs: *mut u16,
            fs_len: u32,
        ) -> i32;
    }
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let w: Vec<u16> = std::ffi::OsStr::new(&root).encode_wide().chain(Some(0)).collect();
        let t = unsafe { GetDriveTypeW(w.as_ptr()) };
        if t != 2 && t != 3 {
            continue; // removable and fixed only
        }
        let mut name = [0u16; 256];
        let mut fs = [0u16; 64];
        let (mut s, mut m, mut f) = (0, 0, 0);
        let ok = unsafe { GetVolumeInformationW(w.as_ptr(), name.as_mut_ptr(), 256, &mut s, &mut m, &mut f, fs.as_mut_ptr(), 64) };
        if ok == 0 {
            continue;
        }
        let z = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
        let (total, free) = walk::win::disk_space(Path::new(&root)).unwrap_or((0, 0));
        out.push(Drive { label: z(&name), fs: z(&fs), root, total, free });
    }
    out
}

#[cfg(not(windows))]
fn drives() -> Vec<Drive> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    vec![Drive { root: home, label: "Home".into(), total: 0, free: 0, fs: "".into() }]
}

#[tauri::command]
fn list_drives() -> Vec<Drive> {
    drives()
}

#[derive(Serialize)]
struct ScanInfo {
    root: String,
    total: u64,
    alloc: u64,
    files: u32,
    items: usize,
    scan_ms: u64,
    method: String,
    via_service: bool,
    volume_total: u64,
    volume_free: u64,
    scanned_at: i64,
    has_previous: bool,
    skipped: u32,
    from_snapshot: bool,
}

fn info_of(idx: &Index, via_service: bool, from_snapshot: bool) -> ScanInfo {
    let has_previous = snapshot::list(&snapshot::dir_for(&snapshot::default_base(), &idx.root_path))
        .iter()
        .any(|(ts, _)| *ts < idx.scanned_at);
    ScanInfo {
        root: idx.root_path.clone(),
        total: idx.total_size[0],
        alloc: idx.total_alloc[0],
        files: idx.file_count[0],
        items: idx.len(),
        scan_ms: idx.scan_ms,
        method: format!("{:?}", idx.method.unwrap_or(gigamap_core::ScanMethod::Walk)),
        via_service,
        volume_total: idx.volume_total,
        volume_free: idx.volume_free,
        scanned_at: idx.scanned_at,
        has_previous,
        skipped: idx.skipped,
        from_snapshot,
    }
}

struct ScanGuard<'a>(&'a AtomicBool);
impl Drop for ScanGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn drive_letter(target: &str) -> Option<char> {
    let t = target.trim_end_matches(['\\', '/']);
    let mut c = t.chars();
    match (c.next(), c.next(), c.next()) {
        (Some(l), Some(':'), None) if l.is_ascii_alphabetic() => Some(l.to_ascii_uppercase()),
        _ => None,
    }
}

fn do_scan(target: &str) -> anyhow::Result<(Index, bool)> {
    if cfg!(windows) {
        if let Some(l) = drive_letter(target) {
            if let Ok(i) = ipc::scan_via_service(l) {
                return Ok((i, true));
            }
            if let Ok(i) = mft::scan_drive(l) {
                return Ok((i, false));
            }
        }
    }
    Ok((walk::scan(Path::new(target))?, false))
}

#[tauri::command]
async fn scan(target: String, st: St<'_>) -> Res<ScanInfo> {
    if st.scanning.swap(true, Ordering::SeqCst) {
        return Err("a scan is already running".into());
    }
    let _guard = ScanGuard(&st.scanning);
    let (idx, via_service) = tauri::async_runtime::spawn_blocking(move || {
        let (idx, svc) = do_scan(&target)?;
        let _ = snapshot::store(&snapshot::default_base(), &idx);
        anyhow::Ok((idx, svc))
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let info = info_of(&idx, via_service, false);
    *st.idx.write().unwrap() = Some(Arc::new(idx));
    Ok(info)
}

/// Open the newest saved scan of a root instantly (no disk walk). None if never scanned.
#[tauri::command]
async fn open_last(root: String, st: St<'_>) -> Res<Option<ScanInfo>> {
    let loaded = tauri::async_runtime::spawn_blocking(move || {
        let label = match drive_letter(&root) {
            Some(l) => format!("{l}:\\"),
            None => root.clone(),
        };
        snapshot::at_or_before(&snapshot::default_base(), &label, i64::MAX).and_then(|(_, p)| snapshot::load(&p).ok())
    })
    .await
    .map_err(err)?;
    let Some(idx) = loaded else { return Ok(None) };
    let info = info_of(&idx, false, true);
    *st.idx.write().unwrap() = Some(Arc::new(idx));
    Ok(Some(info))
}

/// When each root was last scanned (unix seconds), for the drive list.
#[tauri::command]
fn last_scans(roots: Vec<String>) -> Vec<Option<i64>> {
    let base = snapshot::default_base();
    roots
        .iter()
        .map(|r| {
            let label = match drive_letter(r) {
                Some(l) => format!("{l}:\\"),
                None => r.clone(),
            };
            snapshot::list(&snapshot::dir_for(&base, &label)).last().map(|(ts, _)| *ts)
        })
        .collect()
}

#[tauri::command]
fn tree(node: NodeId, max_nodes: usize, st: St) -> Res<Slice> {
    let idx = current(&st)?;
    Ok(query::slice(&idx, node, max_nodes.clamp(100, 60_000)))
}

#[tauri::command]
fn children(node: NodeId, limit: usize, st: St) -> Res<Vec<Item>> {
    let idx = current(&st)?;
    Ok(query::breakdown(&idx, node, limit).items)
}

#[tauri::command]
fn item(node: NodeId, st: St) -> Res<Item> {
    Ok(current(&st)?.item(node))
}

#[tauri::command]
fn ancestors(node: NodeId, st: St) -> Res<Vec<Item>> {
    let idx = current(&st)?;
    let mut v = Vec::new();
    let mut n = node;
    loop {
        v.push(idx.item(n));
        if n == 0 {
            break;
        }
        n = idx.parent[n as usize];
    }
    v.reverse();
    Ok(v)
}

#[tauri::command]
fn find(path: String, st: St) -> Res<Option<NodeId>> {
    Ok(current(&st)?.find(&path))
}

#[tauri::command]
fn top_files(node: NodeId, limit: usize, st: St) -> Res<Vec<Item>> {
    Ok(query::top_files(&*current(&st)?, node, limit, 0))
}

#[tauri::command]
fn hotspots(node: NodeId, limit: usize, st: St) -> Res<Vec<Item>> {
    Ok(query::hotspots(&*current(&st)?, node, limit))
}

#[tauri::command]
fn file_types(node: NodeId, limit: usize, st: St) -> Res<Vec<TypeStat>> {
    Ok(query::file_types(&*current(&st)?, node, limit))
}

#[tauri::command]
fn ext_folders(node: NodeId, ext: String, limit: usize, st: St) -> Res<Vec<ExtFolder>> {
    Ok(query::ext_folders(&*current(&st)?, node, &ext, limit))
}

#[tauri::command]
fn search(node: NodeId, pattern: String, limit: usize, st: St) -> Res<Vec<Item>> {
    Ok(query::search(&*current(&st)?, node, &pattern, limit))
}

#[tauri::command]
fn cleanup_candidates(node: NodeId, st: St) -> Res<Vec<Candidate>> {
    Ok(cleanup::candidates(&*current(&st)?, node, 16 << 20))
}

#[tauri::command]
async fn growth(since: String, st: St<'_>) -> Res<Option<Growth>> {
    let idx = current(&st)?;
    tauri::async_runtime::spawn_blocking(move || {
        let base = snapshot::default_base();
        let cutoff = match since.as_str() {
            "day" => idx.scanned_at - 86_400,
            "week" => idx.scanned_at - 7 * 86_400,
            "month" => idx.scanned_at - 30 * 86_400,
            _ => idx.scanned_at - 1,
        };
        let old = snapshot::at_or_before(&base, &idx.root_path, cutoff)
            .or_else(|| (since != "last").then(|| snapshot::list(&snapshot::dir_for(&base, &idx.root_path)).into_iter().find(|(t, _)| *t < idx.scanned_at)).flatten())
            .and_then(|(_, p)| snapshot::load(&p).ok());
        old.map(|o| diff::growth(&o, &idx, 25))
    })
    .await
    .map_err(err)
}

#[tauri::command]
async fn duplicates(node: NodeId, min_mb: u64, st: St<'_>) -> Res<Vec<DupeGroup>> {
    let idx = current(&st)?;
    tauri::async_runtime::spawn_blocking(move || dupes::find(&idx, node, min_mb << 20, 50))
        .await
        .map_err(err)
}

#[tauri::command]
fn reveal(path: String) -> Res<()> {
    #[cfg(windows)]
    {
        // Shell API opens the parent folder with the item selected; no command-line quoting involved.
        let p = path.clone();
        let ok = std::thread::spawn(move || shell_select(&p)).join().unwrap_or(false);
        if !ok {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("explorer.exe")
                .raw_arg(format!("/select,\"{}\"", path.trim_end_matches('\\')))
                .spawn()
                .map_err(err)?;
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(windows)]
fn shell_select(path: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut core::ffi::c_void, coinit: u32) -> i32;
        fn CoUninitialize();
    }
    #[link(name = "shell32")]
    extern "system" {
        fn ILCreateFromPathW(path: *const u16) -> *mut core::ffi::c_void;
        fn ILFree(pidl: *mut core::ffi::c_void);
        fn SHOpenFolderAndSelectItems(folder: *const core::ffi::c_void, count: u32, items: *const *const core::ffi::c_void, flags: u32) -> i32;
    }
    let w: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().chain(Some(0)).collect();
    unsafe {
        let hr_init = CoInitializeEx(std::ptr::null_mut(), 0x2); // apartment-threaded
        let pidl = ILCreateFromPathW(w.as_ptr());
        let ok = if pidl.is_null() {
            false
        } else {
            let hr = SHOpenFolderAndSelectItems(pidl, 0, std::ptr::null(), 0);
            ILFree(pidl);
            hr >= 0
        };
        if hr_init >= 0 {
            CoUninitialize();
        }
        ok
    }
}

#[tauri::command]
fn recycle(paths: Vec<String>) -> Res<()> {
    for p in &paths {
        requests::check_path(p).map_err(err)?;
    }
    requests::recycle(&paths).map_err(err)
}

#[tauri::command]
fn pending_requests() -> Vec<DeleteRequest> {
    requests::pending()
}

#[tauri::command]
fn resolve_request(id: String, approve: bool) -> Res<DeleteRequest> {
    requests::resolve(&id, approve).map_err(err)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_drives,
            scan,
            open_last,
            last_scans,
            tree,
            children,
            item,
            ancestors,
            find,
            top_files,
            hotspots,
            file_types,
            ext_folders,
            search,
            cleanup_candidates,
            growth,
            duplicates,
            reveal,
            recycle,
            pending_requests,
            resolve_request
        ])
        .run(tauri::generate_context!())
        .expect("error while running Gigamap");
}
