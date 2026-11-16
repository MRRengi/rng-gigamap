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
    /// Nobody answered within a day; never acted on.
    Expired,
}

/// Pending requests older than this are never shown or executed.
pub const EXPIRE_SECS: i64 = 24 * 3600;

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

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn file(id: &str) -> Result<PathBuf> {
    anyhow::ensure!(valid_id(id), "bad request id");
    Ok(dir().join(format!("{id}.json")))
}

/// Refuse anything that must never be bulk-deleted on an agent's say-so: relative paths,
/// wildcards (the shell would expand them), drive roots, Windows and Program Files,
/// the user profile root, NTFS metadata, and paths that do not exist.
pub fn check_path(p: &str) -> Result<()> {
    let t = p.trim();
    anyhow::ensure!(!t.is_empty(), "empty path");
    anyhow::ensure!(!t.contains(['*', '?', '"', '<', '>', '|']), "wildcards and special characters are not allowed: {t}");
    let path = std::path::Path::new(t);
    anyhow::ensure!(path.is_absolute(), "path must be absolute: {t}");
    anyhow::ensure!(path.components().all(|c| !matches!(c, std::path::Component::ParentDir)), "'..' is not allowed: {t}");
    anyhow::ensure!(path.parent().is_some() && path.file_name().is_some(), "refusing a drive root: {t}");
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    anyhow::ensure!(!(name.starts_with('$') && is_root_child(path)), "refusing NTFS metadata or a system folder: {t}");
    let lower = t.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    let protected = ["\\windows", "\\program files", "\\program files (x86)", "\\programdata", "\\users", "\\system volume information", "\\recovery", "\\boot"];
    if is_root_child(path) {
        let tail = &lower[lower.find(['\\', '/']).unwrap_or(0)..].replace('/', "\\");
        anyhow::ensure!(!protected.contains(&tail.as_str()), "refusing a system folder: {t}");
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let h = home.to_string_lossy().trim_end_matches(['\\', '/']).to_ascii_lowercase();
        anyhow::ensure!(lower != h, "refusing the user profile folder: {t}");
    }
    anyhow::ensure!(std::fs::symlink_metadata(path).is_ok(), "does not exist: {t}");
    Ok(())
}

fn is_root_child(p: &std::path::Path) -> bool {
    p.parent().is_some_and(|q| q.parent().is_none())
}

pub fn submit(paths: Vec<String>, sizes: Vec<u64>, reason: &str) -> Result<DeleteRequest> {
    anyhow::ensure!(!paths.is_empty() && paths.len() <= 200, "send between 1 and 200 paths");
    for p in &paths {
        check_path(p)?;
    }
    std::fs::create_dir_all(dir())?;
    cleanup_old();
    let now = crate::index::now_unix();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let id = format!("{now}-{:08x}", nanos ^ std::process::id().rotate_left(16));
    let reason: String = reason.chars().take(300).collect();
    let r = DeleteRequest { id, paths, sizes, reason, created: now, state: State::Pending, message: String::new() };
    write(&r)?;
    Ok(r)
}

/// Mark stale pending requests as expired and drop answered ones older than a week.
pub fn cleanup_old() {
    let now = crate::index::now_unix();
    for e in std::fs::read_dir(dir()).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Some(mut r) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<DeleteRequest>(&b).ok()) else {
            continue;
        };
        if r.state == State::Pending && now - r.created > EXPIRE_SECS {
            r.state = State::Expired;
            let _ = write(&r);
        } else if r.state != State::Pending && now - r.created > 7 * 86_400 {
            let _ = std::fs::remove_file(&p);
        }
    }
}

pub fn write(r: &DeleteRequest) -> Result<()> {
    let f = file(&r.id)?;
    let tmp = f.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(r)?)?;
    std::fs::rename(tmp, f)?;
    Ok(())
}

pub fn get(id: &str) -> Result<DeleteRequest> {
    let b = std::fs::read(file(id)?).with_context(|| format!("no request {id}"))?;
    Ok(serde_json::from_slice(&b)?)
}

pub fn pending() -> Vec<DeleteRequest> {
    let mut v: Vec<DeleteRequest> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .filter(|r: &DeleteRequest| r.state == State::Pending && crate::index::now_unix() - r.created <= EXPIRE_SECS)
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
    if crate::index::now_unix() - r.created > EXPIRE_SECS {
        r.state = State::Expired;
        write(&r)?;
        return Ok(r);
    }
    if !approve {
        r.state = State::Declined;
        write(&r)?;
        return Ok(r);
    }
    r.state = State::Approved;
    write(&r)?;
    let checked: Result<()> = r.paths.iter().try_for_each(|p| check_path(p));
    match checked.and_then(|_| recycle(&r.paths)) {
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

#[cfg(test)]
mod tests {
    use super::check_path;

    #[test]
    fn refuses_dangerous_paths() {
        let tmp = std::env::temp_dir().join(format!("gm-req-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let ok = tmp.join("cache");
        std::fs::create_dir_all(&ok).unwrap();
        assert!(check_path(ok.to_str().unwrap()).is_ok());
        assert!(check_path(&format!("{}/*", tmp.display())).is_err());
        assert!(check_path("relative/dir").is_err());
        assert!(check_path("/").is_err());
        assert!(check_path(&format!("{}/missing", tmp.display())).is_err());
        assert!(check_path(&format!("{}/../etc", tmp.display())).is_err());
        assert!(check_path("/$Extend").is_err());
        if cfg!(windows) {
            assert!(check_path("C:\\Windows").is_err());
            assert!(check_path("C:\\").is_err());
        }
        std::fs::remove_dir_all(&tmp).ok();
    }
}
