//! Binary snapshot of an Index (zstd-compressed). Loading is much faster than rescanning,
//! and two snapshots give "what grew".

use crate::index::{Index, ScanMethod};
use anyhow::{bail, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 5] = b"GMAP\x01";

pub fn save(idx: &Index, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let f = std::fs::File::create(&tmp)?;
        write_to(idx, std::io::BufWriter::new(f))?.flush()?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Serialize into any writer (file, pipe). Returns the inner writer.
pub fn write_to<W: Write>(idx: &Index, out: W) -> Result<W> {
    {
        let mut w = zstd::Encoder::new(out, 3)?;
        w.write_all(MAGIC)?;
        put_str(&mut w, &idx.root_path)?;
        put_u32(&mut w, idx.sep as u32)?;
        w.write_all(&[match idx.method {
            Some(ScanMethod::Mft) => 1,
            Some(ScanMethod::Walk) => 2,
            None => 0,
        }])?;
        for v in [idx.scanned_at as u64, idx.scan_ms, idx.volume_total, idx.volume_free] {
            put_u64(&mut w, v)?;
        }
        put_u64(&mut w, idx.len() as u64)?;
        put_str(&mut w, &idx.names)?;
        put_slice(&mut w, &idx.parent)?;
        put_slice(&mut w, &idx.name_off)?;
        put_slice(&mut w, &idx.name_len)?;
        put_slice(&mut w, &idx.size)?;
        put_slice(&mut w, &idx.alloc)?;
        put_slice(&mut w, &idx.mtime)?;
        w.write_all(&idx.flags)?;
        Ok(w.finish()?)
    }
}

pub fn load(path: &Path) -> Result<Index> {
    let f = std::fs::File::open(path)?;
    read_from(std::io::BufReader::new(f))
}

pub fn read_from<R: Read>(input: R) -> Result<Index> {
    let mut r = zstd::Decoder::new(input)?;
    let mut m = [0u8; 5];
    r.read_exact(&mut m)?;
    if &m != MAGIC {
        bail!("not a Gigamap snapshot");
    }
    let mut idx = Index { root_path: get_str(&mut r)?, ..Default::default() };
    idx.sep = char::from_u32(get_u32(&mut r)?).unwrap_or('\\');
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    idx.method = match b[0] {
        1 => Some(ScanMethod::Mft),
        2 => Some(ScanMethod::Walk),
        _ => None,
    };
    idx.scanned_at = get_u64(&mut r)? as i64;
    idx.scan_ms = get_u64(&mut r)?;
    idx.volume_total = get_u64(&mut r)?;
    idx.volume_free = get_u64(&mut r)?;
    let n = get_u64(&mut r)? as usize;
    idx.names = get_str(&mut r)?;
    idx.parent = get_slice(&mut r, n)?;
    idx.name_off = get_slice(&mut r, n)?;
    idx.name_len = get_slice(&mut r, n)?;
    idx.size = get_slice(&mut r, n)?;
    idx.alloc = get_slice(&mut r, n)?;
    idx.mtime = get_slice(&mut r, n)?;
    idx.flags = vec![0u8; n];
    r.read_exact(&mut idx.flags)?;
    idx.finalize();
    Ok(idx)
}

/// Where snapshots for a root live: `<base>/<sanitized root>/<unix>.gmap`.
pub fn dir_for(base: &Path, root_path: &str) -> PathBuf {
    let key: String = root_path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    base.join(key.trim_matches('_').to_string() + "_")
}

pub fn list(dir: &Path) -> Vec<(i64, PathBuf)> {
    let mut v: Vec<(i64, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ts = p.file_stem()?.to_str()?.parse::<i64>().ok()?;
            (p.extension()? == "gmap").then_some((ts, p))
        })
        .collect();
    v.sort();
    v
}

/// Save a new snapshot and keep the newest `keep`.
pub fn store(base: &Path, idx: &Index, keep: usize) -> Result<PathBuf> {
    let dir = dir_for(base, &idx.root_path);
    let p = dir.join(format!("{}.gmap", idx.scanned_at));
    save(idx, &p)?;
    let all = list(&dir);
    if all.len() > keep {
        for (_, old) in &all[..all.len() - keep] {
            std::fs::remove_file(old).ok();
        }
    }
    Ok(p)
}

pub fn default_base() -> PathBuf {
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(l).join("Gigamap").join("snapshots");
    }
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h).join(".local/share/gigamap/snapshots");
    }
    std::env::temp_dir().join("gigamap/snapshots")
}

fn put_u32(w: &mut impl Write, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn put_u64(w: &mut impl Write, v: u64) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn put_str(w: &mut impl Write, s: &str) -> std::io::Result<()> {
    put_u64(w, s.len() as u64)?;
    w.write_all(s.as_bytes())
}
fn get_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn get_u64(r: &mut impl Read) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn get_str(r: &mut impl Read) -> Result<String> {
    let n = get_u64(r)? as usize;
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    Ok(String::from_utf8(b)?)
}

trait Pod: Copy + Default {
    const N: usize;
    fn to(self, out: &mut Vec<u8>);
    fn from(b: &[u8]) -> Self;
}
macro_rules! pod {
    ($($t:ty),*) => {$(
        impl Pod for $t {
            const N: usize = std::mem::size_of::<$t>();
            fn to(self, out: &mut Vec<u8>) { out.extend_from_slice(&self.to_le_bytes()) }
            fn from(b: &[u8]) -> Self { <$t>::from_le_bytes(b.try_into().unwrap()) }
        }
    )*};
}
pod!(u16, u32, u64, i64);

fn put_slice<T: Pod>(w: &mut impl Write, v: &[T]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(v.len().min(1 << 20) * T::N);
    for chunk in v.chunks(1 << 20) {
        buf.clear();
        for &x in chunk {
            x.to(&mut buf);
        }
        w.write_all(&buf)?;
    }
    Ok(())
}
fn get_slice<T: Pod>(r: &mut impl Read, n: usize) -> Result<Vec<T>> {
    let mut b = vec![0u8; n * T::N];
    r.read_exact(&mut b)?;
    Ok(b.chunks_exact(T::N).map(T::from).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip() {
        let mut b = crate::IndexBuilder::new("C:\\", '\\', crate::ScanMethod::Mft);
        let d = b.push(0, "Папка", 0, 0, 5, crate::index::flag::DIR);
        b.push(d, "f.bin", 42, 4096, 7, 0);
        let i = b.finish(std::time::Instant::now());
        let p = std::env::temp_dir().join(format!("gm-snap-{}.gmap", std::process::id()));
        super::save(&i, &p).unwrap();
        let j = super::load(&p).unwrap();
        assert_eq!(j.total_size[0], 42);
        assert_eq!(j.path(2), "C:\\Папка\\f.bin");
        std::fs::remove_file(p).ok();
    }
}
