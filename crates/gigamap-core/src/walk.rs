//! Parallel directory walker: fallback for non-NTFS volumes and non-elevated runs.
//! One lock per directory (not per file), symlinks and junctions are not followed.

use crate::index::{flag, Index, IndexBuilder, NodeId, ScanMethod};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

struct Entry {
    name: String,
    size: u64,
    alloc: u64,
    mtime: i64,
    flags: u8,
    dir_path: Option<PathBuf>,
}

pub fn scan(root: &Path) -> Result<Index> {
    let started = Instant::now();
    let meta = fs::metadata(root).with_context(|| format!("reading {}", root.display()))?;
    anyhow::ensure!(meta.is_dir(), "{} is not a folder", root.display());
    let root_str = root.to_string_lossy().to_string();
    let sep = if cfg!(windows) { '\\' } else { '/' };
    let builder = Mutex::new(IndexBuilder::new(&root_str, sep, ScanMethod::Walk));
    builder.lock().unwrap().idx.mtime[0] = mtime_of(&meta);
    rayon::scope(|s| visit(s, &builder, root.to_path_buf(), 0));
    let mut b = builder.into_inner().unwrap();
    let (total, free) = volume_space(root);
    b.idx.volume_total = total;
    b.idx.volume_free = free;
    Ok(b.finish(started))
}

fn visit<'s>(s: &rayon::Scope<'s>, builder: &'s Mutex<IndexBuilder>, dir: PathBuf, id: NodeId) {
    let Ok(rd) = fs::read_dir(&dir) else { return };
    let mut entries: Vec<Entry> = Vec::new();
    for de in rd.flatten() {
        let Ok(ft) = de.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let Ok(md) = de.metadata() else { continue };
        let name = de.file_name().to_string_lossy().to_string();
        let is_dir = ft.is_dir();
        let mut fl = if is_dir { flag::DIR } else { 0 };
        fl |= attr_flags(&md);
        let size = if is_dir { 0 } else { md.len() };
        entries.push(Entry {
            alloc: if is_dir { 0 } else { alloc_of(&md) },
            size,
            mtime: mtime_of(&md),
            flags: fl,
            dir_path: is_dir.then(|| de.path()),
            name,
        });
    }
    let mut subdirs: Vec<(PathBuf, NodeId)> = Vec::new();
    {
        let mut b = builder.lock().unwrap();
        for e in entries {
            let child = b.push(id, &e.name, e.size, e.alloc, e.mtime, e.flags);
            if let Some(p) = e.dir_path {
                subdirs.push((p, child));
            }
        }
    }
    for (p, child) in subdirs {
        s.spawn(move |s| visit(s, builder, p, child));
    }
}

fn mtime_of(md: &fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(unix)]
fn alloc_of(md: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    md.blocks() * 512
}
#[cfg(not(unix))]
fn alloc_of(md: &fs::Metadata) -> u64 {
    md.len()
}

#[cfg(windows)]
fn attr_flags(md: &fs::Metadata) -> u8 {
    use std::os::windows::fs::MetadataExt;
    let a = md.file_attributes();
    let mut f = 0;
    if a & 0x2 != 0 {
        f |= flag::HIDDEN;
    }
    if a & 0x4 != 0 {
        f |= flag::SYSTEM;
    }
    f
}
#[cfg(not(windows))]
fn attr_flags(_md: &fs::Metadata) -> u8 {
    0
}

#[cfg(windows)]
fn volume_space(root: &Path) -> (u64, u64) {
    crate::walk::win::disk_space(root).unwrap_or((0, 0))
}
#[cfg(not(windows))]
fn volume_space(_root: &Path) -> (u64, u64) {
    (0, 0)
}

#[cfg(windows)]
pub mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }

    pub fn disk_space(root: &Path) -> Option<(u64, u64)> {
        let w: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
        let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) };
        (ok != 0).then_some((t, f))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn walks_temp_tree() {
        let d = std::env::temp_dir().join(format!("gm-walk-{}", std::process::id()));
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("a/b/x.bin"), vec![0u8; 3000]).unwrap();
        std::fs::write(d.join("y.txt"), b"hello").unwrap();
        let i = super::scan(&d).unwrap();
        assert_eq!(i.total_size[0], 3005);
        assert_eq!(i.file_count[0], 2);
        assert!(i.find("a/b/x.bin").is_some());
        std::fs::remove_dir_all(&d).ok();
    }
}
