//! Picks the fastest way to scan a target: NTFS MFT when possible, parallel walk otherwise.

use anyhow::Result;
use gigamap_core::{mft, walk, Index};
use std::path::Path;

/// "C", "C:", "C:\" -> Some('C')
pub fn drive_letter(target: &str) -> Option<char> {
    let t = target.trim_end_matches(['\\', '/']);
    let mut c = t.chars();
    match (c.next(), c.next(), c.next()) {
        (Some(l), Some(':'), None) | (Some(l), None, None) if l.is_ascii_alphabetic() => Some(l.to_ascii_uppercase()),
        _ => None,
    }
}

pub fn default_target() -> String {
    if cfg!(windows) {
        "C:".into()
    } else {
        "/".into()
    }
}

pub fn scan(target: &str, image: Option<&str>) -> Result<Index> {
    if let Some(img) = image {
        let label = match drive_letter(target) {
            Some(l) => format!("{l}:\\"),
            None => target.to_string(),
        };
        return mft::scan_path(img, &label, '\\');
    }
    if cfg!(windows) {
        if let Some(l) = drive_letter(target) {
            if let Ok(i) = gigamap_core::ipc::scan_via_service(l) {
                return Ok(i);
            }
            match mft::scan_drive(l) {
                Ok(i) => return Ok(i),
                Err(e) => eprintln!("MFT scan unavailable ({e:#}), walking folders instead"),
            }
            return walk::scan(Path::new(&format!("{l}:\\")));
        }
    }
    walk::scan(Path::new(target))
}
