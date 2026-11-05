//! Delete requests from agents. An agent can only *ask*; the Gigamap window shows
//! a confirmation toast, and only an approved request moves files to the Recycle Bin.
//! Requests are small JSON files so any process (MCP server, UI, service) can share them.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Pending,
    Approved,
    Declined,
    Done,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeleteRequest {
    pub id: String,
    pub paths: Vec<String>,
    pub sizes: Vec<u64>,
    pub reason: String,
    pub created: i64,
    pub state: State,
    #[serde(default)]
    pub message: String,
}

pub fn dir() -> PathBuf {
    let base = std::env::var("GIGAMAP_HOME").map(PathBuf::from).unwrap_or_else(|_| {
        crate::snapshot::default_base().parent().map(|p| p.to_path_buf()).unwrap_or_else(std::env::temp_dir)
    });
    base.join("requests")
}

fn file(id: &str) -> PathBuf {
    dir().join(format!("{id}.json"))
}

pub fn submit(paths: Vec<String>, sizes: Vec<u64>, reason: &str) -> Result<DeleteRequest> {
    std::fs::create_dir_all(dir())?;
    let now = crate::index::now_unix();
    let id = format!("{now}-{:04x}", std::process::id() as u16 ^ (now as u16).rotate_left(5));
    let r = DeleteRequest { id, paths, sizes, reason: reason.to_string(), created: now, state: State::Pending, message: String::new() };
    write(&r)?;
    Ok(r)
}

pub fn write(r: &DeleteRequest) -> Result<()> {
    let tmp = file(&r.id).with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(r)?)?;
    std::fs::rename(tmp, file(&r.id))?;
    Ok(())
}

pub fn get(id: &str) -> Result<DeleteRequest> {
    let b = std::fs::read(file(id)).with_context(|| format!("no request {id}"))?;
    Ok(serde_json::from_slice(&b)?)
}

pub fn pending() -> Vec<DeleteRequest> {
    let mut v: Vec<DeleteRequest> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .filter(|r: &DeleteRequest| r.state == State::Pending)
        .collect();
    v.sort_by_key(|r| r.created);
    v
}

/// Called by the UI after the user answers the toast.
pub fn resolve(id: &str, approve: bool) -> Result<DeleteRequest> {
    let mut r = get(id)?;
    if r.state != State::Pending {
        return Ok(r);
    }
    if !approve {
        r.state = State::Declined;
        write(&r)?;
        return Ok(r);
    }
    r.state = State::Approved;
    write(&r)?;
    match recycle(&r.paths) {
        Ok(()) => r.state = State::Done,
        Err(e) => {
            r.state = State::Failed;
            r.message = format!("{e:#}");
        }
    }
    write(&r)?;
    Ok(r)
}

/// Move to the Recycle Bin (never a permanent delete).
#[cfg(windows)]
pub fn recycle(paths: &[String]) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[repr(C)]
    struct ShFileOpStructW {
        hwnd: isize,
        w_func: u32,
        p_from: *const u16,
        p_to: *const u16,
        f_flags: u16,
        f_any_operations_aborted: i32,
        h_name_mappings: *mut core::ffi::c_void,
        lpsz_progress_title: *const u16,
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHFileOperationW(op: *mut ShFileOpStructW) -> i32;
    }
    const FO_DELETE: u32 = 3;
    const FOF_SILENT: u16 = 0x4;
    const FOF_NOCONFIRMATION: u16 = 0x10;
    const FOF_ALLOWUNDO: u16 = 0x40;
    const FOF_NOERRORUI: u16 = 0x400;
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(std::ffi::OsStr::new(p).encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut op = ShFileOpStructW {
        hwnd: 0,
        w_func: FO_DELETE,
        p_from: from.as_ptr(),
        p_to: std::ptr::null(),
        f_flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };
    let rc = unsafe { SHFileOperationW(&mut op) };
    anyhow::ensure!(rc == 0 && op.f_any_operations_aborted == 0, "Recycle Bin move failed (code {rc})");
    Ok(())
}

#[cfg(not(windows))]
pub fn recycle(_paths: &[String]) -> Result<()> {
    anyhow::bail!("Recycle Bin is only supported on Windows")
}
