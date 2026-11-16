//! Direct NTFS Master File Table reader.
//!
//! Works on any `Read + Seek` source: a raw volume handle (`\\.\C:`, needs
//! admin) or an image file. Reads the `$MFT` sequentially in big chunks and
//! parses FILE records in parallel.

use crate::index::{flag, Index, IndexBuilder, ScanMethod, NONE};
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use std::io::{Read, Seek, SeekFrom};
use std::time::Instant;

const CHUNK: usize = 16 * 1024 * 1024;
const ROOT_RECORD: u64 = 5;
const BITMAP_RECORD: u64 = 6;
const BADCLUS_RECORD: u32 = 8;
const EXTEND_RECORD: u64 = 11;

#[derive(Clone, Copy, Debug)]
pub struct Boot {
    pub bytes_per_sector: u64,
    pub cluster_size: u64,
    pub mft_lcn: u64,
    pub record_size: usize,
    pub total_bytes: u64,
}

pub fn parse_boot(b: &[u8]) -> Result<Boot> {
    if b.len() < 512 || &b[3..11] != b"NTFS    " {
        bail!("not an NTFS volume");
    }
    let bps = u16le(b, 0x0B) as u64;
    let raw = b[0x0D];
    let spc: u64 = if raw <= 0x80 { raw as u64 } else { 1u64 << (256 - raw as u32) };
    if bps == 0 || spc == 0 {
        bail!("bad NTFS geometry");
    }
    let cluster_size = bps * spc;
    let cpr = b[0x40] as i8;
    let record_size = if cpr > 0 { cpr as u64 * cluster_size } else { 1u64 << (-(cpr as i32)) } as usize;
    if !(256..=65536).contains(&record_size) {
        bail!("bad MFT record size {record_size}");
    }
    Ok(Boot {
        bytes_per_sector: bps,
        cluster_size,
        mft_lcn: u64le(b, 0x30),
        record_size,
        total_bytes: u64le(b, 0x28) * bps,
    })
}

/// One data run: `lcn == None` means sparse.
#[derive(Clone, Copy, Debug)]
pub struct Run {
    pub lcn: Option<u64>,
    pub clusters: u64,
}

pub fn parse_runs(buf: &[u8]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut p = 0usize;
    let mut prev: i64 = 0;
    while p < buf.len() {
        let h = buf[p];
        if h == 0 {
            break;
        }
        let ls = (h & 0x0F) as usize;
        let os = (h >> 4) as usize;
        p += 1;
        if ls == 0 || ls > 8 || os > 8 || p + ls + os > buf.len() {
            break;
        }
        let mut len: u64 = 0;
        for i in 0..ls {
            len |= (buf[p + i] as u64) << (8 * i);
        }
        p += ls;
        if os == 0 {
            runs.push(Run { lcn: None, clusters: len });
        } else {
            let mut off: i64 = 0;
            for i in 0..os {
                off |= (buf[p + i] as i64) << (8 * i);
            }
            if buf[p + os - 1] & 0x80 != 0 && os < 8 {
                off |= -1i64 << (8 * os);
            }
            p += os;
            prev += off;
            runs.push(Run { lcn: Some(prev as u64), clusters: len });
        }
    }
    runs
}

/// Undo the update sequence array. Returns false for torn/corrupt records.
pub fn apply_fixup(rec: &mut [u8]) -> bool {
    let usa_off = u16le(rec, 4) as usize;
    let usa_cnt = u16le(rec, 6) as usize;
    if usa_cnt == 0 {
        return true;
    }
    if usa_off + 2 * usa_cnt > rec.len() {
        return false;
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_cnt {
        let pos = i * 512 - 2;
        if pos + 2 > rec.len() {
            break;
        }
        if rec[pos] != usn[0] || rec[pos + 1] != usn[1] {
            return false;
        }
        rec[pos] = rec[usa_off + 2 * i];
        rec[pos + 1] = rec[usa_off + 2 * i + 1];
    }
    true
}

/// One parsed FILE record, kept small: millions of these live at once during a scan.
#[derive(Default, Debug)]
pub struct Rec {
    pub number: u32,
    /// Base record for extension records, 0 for base records.
    pub base: u64,
    pub seq: u16,
    pub is_dir: bool,
    pub attrs: u32,
    pub mtime: i64,
    /// Best name seen so far (Win32 over POSIX over DOS) and its parent reference.
    pub name: Box<str>,
    pub name_rank: u8,
    pub parent: u64,
    pub parent_seq: u16,
    pub size: u64,
    pub alloc: u64,
}

pub const NO_NAME: u8 = u8::MAX;

fn name_rank(ns: u8) -> u8 {
    match ns {
        1 | 3 => 0,
        0 => 1,
        _ => 2,
    }
}

/// Parse one FILE record (fixups already applied). None if unused/invalid.
pub fn parse_record(rec: &[u8]) -> Option<Rec> {
    if rec.len() < 0x30 || &rec[0..4] != b"FILE" {
        return None;
    }
    let flags = u16le(rec, 0x16);
    if flags & 1 == 0 {
        return None;
    }
    let mut r = Rec {
        number: u32le(rec, 0x2C),
        base: u64le(rec, 0x20) & 0x0000_FFFF_FFFF_FFFF,
        seq: u16le(rec, 0x10),
        is_dir: flags & 2 != 0,
        name_rank: NO_NAME,
        ..Default::default()
    };
    for (typ, nonres, a) in attributes(rec) {
        let name_len = a[9] as usize;
        let aflags = u16le(a, 0x0C);
        match (typ, nonres) {
            (0x10, false) => {
                if let Some(v) = resident_value(a) {
                    if v.len() >= 0x24 {
                        r.mtime = filetime_to_unix(u64le(v, 0x08));
                        r.attrs = u32le(v, 0x20);
                    }
                }
            }
            (0x30, false) => {
                if let Some(v) = resident_value(a) {
                    if v.len() >= 0x42 {
                        let pref = u64le(v, 0);
                        let nl = v[0x40] as usize;
                        let rank = name_rank(v[0x41]);
                        if rank < r.name_rank && v.len() >= 0x42 + nl * 2 {
                            let units: Vec<u16> = (0..nl).map(|i| u16le(v, 0x42 + i * 2)).collect();
                            r.name = String::from_utf16_lossy(&units).into_boxed_str();
                            r.name_rank = rank;
                            r.parent = pref & 0x0000_FFFF_FFFF_FFFF;
                            r.parent_seq = (pref >> 48) as u16;
                        }
                    }
                }
            }
            (0x80, _) => {
                let named = name_len > 0;
                if named && r.number == BADCLUS_RECORD {
                    // $BadClus:$Bad is a sparse stream as big as the volume.
                } else if !nonres {
                    r.size += u32le(a, 0x10) as u64;
                } else if a.len() >= 0x40 && u64le(a, 0x10) == 0 {
                    let alloc_sz = u64le(a, 0x28);
                    let real = u64le(a, 0x30);
                    let compressed_or_sparse = aflags & 0x0001 != 0 || aflags & 0x8000 != 0;
                    let runlist_off = u16le(a, 0x20) as usize;
                    let on_disk = if compressed_or_sparse && a.len() >= 0x48 && runlist_off >= 0x48 {
                        u64le(a, 0x40)
                    } else {
                        alloc_sz
                    };
                    r.alloc += on_disk;
                    if !named || aflags & 0x8000 == 0 {
                        r.size += real;
                    }
                }
            }
            (0xA0, true) => {
                if a.len() >= 0x40 && u64le(a, 0x10) == 0 {
                    r.alloc += u64le(a, 0x28);
                }
            }
            _ => {}
        }
    }
    Some(r)
}

/// Walk the attributes of a FILE record: (type, non-resident, bytes).
fn attributes(rec: &[u8]) -> impl Iterator<Item = (u32, bool, &[u8])> {
    let mut p = u16le(rec, 0x14) as usize;
    std::iter::from_fn(move || {
        if p + 16 > rec.len() {
            return None;
        }
        let typ = u32le(rec, p);
        let len = u32le(rec, p + 4) as usize;
        if typ == 0xFFFF_FFFF || len < 16 || p + len > rec.len() {
            return None;
        }
        let a = &rec[p..p + len];
        p += len;
        Some((typ, a[8] != 0, a))
    })
}

/// What we need from the `$MFT`/`$Bitmap` records themselves: the unnamed `$DATA`
/// runs (with their starting VCN) and the attribute list, if the stream is split.
#[derive(Default, Debug)]
pub struct DataInfo {
    pub start_vcn: u64,
    pub runs: Vec<Run>,
    pub size: u64,
    /// (attribute type, start VCN, record number) from `$ATTRIBUTE_LIST`.
    pub attr_list: Vec<(u32, u64, u64)>,
    /// Non-resident attribute list runs and length, if the list itself is not resident.
    pub attr_list_runs: Option<(Vec<Run>, u64)>,
}

pub fn parse_data_info(rec: &[u8]) -> DataInfo {
    let mut d = DataInfo::default();
    for (typ, nonres, a) in attributes(rec) {
        match (typ, nonres) {
            (0x80, true) if a[9] == 0 && a.len() >= 0x40 => {
                let start = u64le(a, 0x10);
                let off = u16le(a, 0x20) as usize;
                if start == 0 {
                    d.size = u64le(a, 0x30);
                }
                if d.runs.is_empty() && off < a.len() {
                    d.start_vcn = start;
                    d.runs = parse_runs(&a[off..]);
                }
            }
            (0x20, false) => {
                if let Some(v) = resident_value(a) {
                    d.attr_list = parse_attr_list(v);
                }
            }
            (0x20, true) if a.len() >= 0x40 => {
                let off = u16le(a, 0x20) as usize;
                if off < a.len() {
                    d.attr_list_runs = Some((parse_runs(&a[off..]), u64le(a, 0x30)));
                }
            }
            _ => {}
        }
    }
    d
}

pub fn parse_attr_list(v: &[u8]) -> Vec<(u32, u64, u64)> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 0x1A <= v.len() {
        let len = u16le(v, p + 4) as usize;
        if len < 0x1A || p + len > v.len() {
            break;
        }
        out.push((u32le(v, p), u64le(v, p + 8), u64le(v, p + 0x10) & 0x0000_FFFF_FFFF_FFFF));
        p += len;
    }
    out
}

fn resident_value(a: &[u8]) -> Option<&[u8]> {
    if a.len() < 0x18 {
        return None;
    }
    let vl = u32le(a, 0x10) as usize;
    let vo = u16le(a, 0x14) as usize;
    a.get(vo..vo.checked_add(vl)?)
}

pub fn filetime_to_unix(ft: u64) -> i64 {
    if ft == 0 {
        return 0;
    }
    (ft / 10_000_000) as i64 - 11_644_473_600
}

pub struct Volume<R> {
    r: R,
    pub boot: Boot,
}

impl<R: Read + Seek + Send> Volume<R> {
    pub fn open(mut r: R) -> Result<Self> {
        let mut b = vec![0u8; 4096];
        r.seek(SeekFrom::Start(0))?;
        read_full(&mut r, &mut b).context("reading boot sector")?;
        let boot = parse_boot(&b)?;
        Ok(Volume { r, boot })
    }

    /// Read records 0..16, which always sit at the start of the MFT.
    fn read_head_records(&mut self) -> Result<Vec<u8>> {
        let rs = self.boot.record_size as u64;
        let cs = self.boot.cluster_size;
        let want = (rs * 16).div_ceil(cs).max(1) * cs;
        let want = want.max(4096);
        let mut buf = vec![0u8; want as usize];
        self.r.seek(SeekFrom::Start(self.boot.mft_lcn * cs))?;
        read_full(&mut self.r, &mut buf).context("reading $MFT head")?;
        Ok(buf)
    }

    fn head_record(head: &[u8], n: u64, rs: usize) -> Result<Vec<u8>> {
        let mut rec = head[n as usize * rs..(n as usize + 1) * rs].to_vec();
        if !apply_fixup(&mut rec) || &rec[0..4] != b"FILE" {
            bail!("record {n} failed fixup");
        }
        Ok(rec)
    }

    /// Read `len` bytes of a non-resident stream described by `runs`.
    fn read_stream(&mut self, runs: &[Run], len: u64) -> Result<Vec<u8>> {
        let cs = self.boot.cluster_size;
        let mut out = Vec::with_capacity(len as usize);
        for run in runs {
            if out.len() as u64 >= len {
                break;
            }
            let bytes = (run.clusters * cs) as usize;
            match run.lcn {
                None => out.resize(out.len() + bytes, 0),
                Some(lcn) => {
                    let mut buf = vec![0u8; bytes];
                    self.r.seek(SeekFrom::Start(lcn * cs))?;
                    read_full(&mut self.r, &mut buf)?;
                    out.extend_from_slice(&buf);
                }
            }
        }
        out.truncate(len as usize);
        Ok(out)
    }

    /// Read one MFT record by number, using the runs known so far.
    fn read_record(&mut self, runs: &[Run], n: u64) -> Result<Vec<u8>> {
        let cs = self.boot.cluster_size;
        let rs = self.boot.record_size as u64;
        let byte = n * rs;
        let mut vcn_byte = 0u64;
        for run in runs {
            let len = run.clusters * cs;
            if byte < vcn_byte + len {
                let lcn = run.lcn.with_context(|| format!("record {n} is in a sparse run"))?;
                let within = byte - vcn_byte;
                // Read whole clusters: raw volumes only accept sector-aligned reads.
                let start = lcn * cs + within / cs * cs;
                let span = ((within % cs) + rs).div_ceil(cs) * cs;
                let mut buf = vec![0u8; span as usize];
                self.r.seek(SeekFrom::Start(start))?;
                read_full(&mut self.r, &mut buf)?;
                let o = (within % cs) as usize;
                let mut rec = buf[o..o + rs as usize].to_vec();
                if !apply_fixup(&mut rec) || &rec[0..4] != b"FILE" {
                    bail!("record {n} failed fixup");
                }
                return Ok(rec);
            }
            vcn_byte += len;
        }
        bail!("record {n} is outside the known MFT runs")
    }

    /// The full run list of `$MFT`, following `$ATTRIBUTE_LIST` when the stream is split
    /// across extension records (heavily fragmented MFTs).
    fn mft_runs(&mut self, head: &[u8]) -> Result<(Vec<Run>, u64)> {
        let rs = self.boot.record_size;
        let rec0 = Self::head_record(head, 0, rs)?;
        let mut info = parse_data_info(&rec0);
        if info.runs.is_empty() {
            bail!("$MFT has no data runs");
        }
        if let Some((runs, len)) = info.attr_list_runs.take() {
            let bytes = self.read_stream(&runs, len)?;
            info.attr_list = parse_attr_list(&bytes);
        }
        let mut pieces: Vec<(u64, Vec<Run>)> = vec![(info.start_vcn, info.runs.clone())];
        let mut extra: Vec<(u64, u64)> = info
            .attr_list
            .iter()
            .filter(|(t, vcn, rec)| *t == 0x80 && *rec != 0 && *vcn != 0)
            .map(|(_, vcn, rec)| (*vcn, *rec))
            .collect();
        extra.sort_unstable();
        extra.dedup();
        for (_, recno) in extra {
            let known: Vec<Run> = {
                let mut v: Vec<(u64, Vec<Run>)> = pieces.clone();
                v.sort_by_key(|p| p.0);
                v.into_iter().flat_map(|p| p.1).collect()
            };
            let rec = self.read_record(&known, recno)?;
            let d = parse_data_info(&rec);
            if !d.runs.is_empty() {
                pieces.push((d.start_vcn, d.runs));
            }
        }
        pieces.sort_by_key(|p| p.0);
        Ok((pieces.into_iter().flat_map(|p| p.1).collect(), info.size))
    }

    /// Count free space from $Bitmap.
    pub fn free_bytes(&mut self) -> Result<u64> {
        let rs = self.boot.record_size;
        let cs = self.boot.cluster_size;
        let head = self.read_head_records()?;
        let bm = parse_data_info(&Self::head_record(&head, BITMAP_RECORD, rs)?);
        let total_clusters = self.boot.total_bytes / cs;
        let mut used: u64 = 0;
        let mut bits_left = total_clusters;
        for run in &bm.runs {
            let Some(lcn) = run.lcn else { continue };
            let mut remaining = run.clusters * cs;
            let mut pos = lcn * cs;
            while remaining > 0 && bits_left > 0 {
                let n = remaining.min(CHUNK as u64) as usize;
                let mut buf = vec![0u8; n];
                self.r.seek(SeekFrom::Start(pos))?;
                read_full(&mut self.r, &mut buf)?;
                for &byte in &buf {
                    if bits_left == 0 {
                        break;
                    }
                    let take = bits_left.min(8) as u32;
                    let mask = if take == 8 { 0xFF } else { (1u16 << take) as u8 - 1 };
                    used += (byte & mask).count_ones() as u64;
                    bits_left -= take as u64;
                }
                remaining -= n as u64;
                pos += n as u64;
            }
        }
        Ok((total_clusters - used.min(total_clusters)) * cs)
    }

    /// Full scan into an index rooted at `root_path`.
    pub fn scan(mut self, root_path: &str, sep: char) -> Result<Index> {
        let started = Instant::now();
        let rs = self.boot.record_size;
        let cs = self.boot.cluster_size;
        let free = self.free_bytes().unwrap_or(0);
        let head = self.read_head_records()?;
        let (runs, mft_size) = self.mft_runs(&head)?;
        let total_records = (mft_size / rs as u64) as usize;
        let mut recs: Vec<Option<Rec>> = Vec::new();
        recs.resize_with(total_records, || None);
        let (tx, rx) = crossbeam_channel::bounded::<(usize, Vec<u8>)>(2);
        let reader = &mut self.r;
        std::thread::scope(|s| -> Result<()> {
            let h = s.spawn(move || -> Result<()> {
                let mut rec_no = 0usize;
                for run in runs {
                    let bytes = run.clusters * cs;
                    let Some(lcn) = run.lcn else {
                        rec_no += (bytes / rs as u64) as usize;
                        continue;
                    };
                    let mut pos = lcn * cs;
                    let mut left = bytes;
                    while left > 0 && rec_no < total_records {
                        let n = left.min(CHUNK as u64) as usize;
                        let mut buf = vec![0u8; n];
                        reader.seek(SeekFrom::Start(pos))?;
                        read_full(reader, &mut buf)?;
                        let cnt = n / rs;
                        if tx.send((rec_no, buf)).is_err() {
                            return Ok(());
                        }
                        rec_no += cnt;
                        pos += n as u64;
                        left -= n as u64;
                    }
                }
                Ok(())
            });
            for (first, mut buf) in rx {
                let parsed: Vec<Option<Rec>> = buf
                    .par_chunks_mut(rs)
                    .map(|chunk| {
                        if chunk.len() < rs || !apply_fixup(chunk) {
                            return None;
                        }
                        parse_record(chunk)
                    })
                    .collect();
                for (i, r) in parsed.into_iter().enumerate() {
                    if first + i < total_records {
                        recs[first + i] = r;
                    }
                }
            }
            h.join().expect("mft reader thread panicked")
        })?;

        // Fold extension records into their base records.
        let mut ext: Vec<(usize, Rec)> = Vec::new();
        for (i, slot) in recs.iter_mut().enumerate() {
            if slot.as_ref().is_some_and(|r| r.base != 0 && r.base as usize != i) {
                let r = slot.take().unwrap();
                ext.push((r.base as usize, r));
            }
        }
        for (b, e) in ext {
            if let Some(Some(base)) = recs.get_mut(b) {
                base.size += e.size;
                base.alloc += e.alloc;
                if e.name_rank < base.name_rank {
                    base.name = e.name;
                    base.name_rank = e.name_rank;
                    base.parent = e.parent;
                    base.parent_seq = e.parent_seq;
                }
            }
        }
        // Sequence numbers let us spot a parent reference that points at a reused record.
        let seq_of: Vec<u16> = recs.iter().map(|r| r.as_ref().map_or(0, |r| r.seq)).collect();

        let mut b = IndexBuilder::new(root_path, sep, ScanMethod::Mft);
        b.reserve(total_records, total_records * 16);
        let mut node_of = vec![NONE; total_records];
        if let Some(Some(root)) = recs.get(ROOT_RECORD as usize) {
            b.idx.mtime[0] = root.mtime;
        }
        node_of[ROOT_RECORD as usize] = 0;
        let mut parent_rec: Vec<u64> = vec![0];
        for (i, slot) in recs.iter().enumerate() {
            let Some(r) = slot else { continue };
            if i as u64 == ROOT_RECORD || r.name_rank == NO_NAME {
                continue;
            }
            let stale = r.parent_seq != 0 && seq_of.get(r.parent as usize).is_some_and(|&s| s != 0 && s != r.parent_seq);
            let parent = if stale { ROOT_RECORD } else { r.parent };
            let name: &str = &r.name;
            let mut fl = 0u8;
            if r.is_dir {
                fl |= flag::DIR;
            }
            if r.attrs & 0x2 != 0 {
                fl |= flag::HIDDEN;
            }
            if r.attrs & 0x4 != 0 {
                fl |= flag::SYSTEM;
            }
            if r.attrs & 0x400 != 0 {
                fl |= flag::REPARSE;
            }
            if i < 16 || parent == EXTEND_RECORD {
                fl |= flag::META;
            }
            let (size, alloc) = if r.is_dir { (0, r.alloc) } else { (r.size, r.alloc) };
            let id = b.push(NONE, name, size, alloc, r.mtime, fl);
            node_of[i] = id;
            parent_rec.push(parent);
        }
        drop(recs);
        for (id, &pr) in parent_rec.iter().enumerate().skip(1) {
            let p = node_of.get(pr as usize).copied().unwrap_or(NONE);
            b.idx.parent[id] = if p == NONE { 0 } else { p };
        }
        b.idx.volume_total = self.boot.total_bytes;
        b.idx.volume_free = free;
        Ok(b.finish(started))
    }
}

fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<()> {
    let mut off = 0;
    while off < buf.len() {
        let n = r.read(&mut buf[off..])?;
        if n == 0 {
            // Short read at end of an image: zero-fill the rest.
            buf[off..].fill(0);
            break;
        }
        off += n;
    }
    Ok(())
}

#[inline]
fn u16le(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}
#[inline]
fn u32le(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}
#[inline]
fn u64le(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8).map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap()))
}

/// Scan an NTFS image file or (on Windows, elevated) a raw volume path.
pub fn scan_path(source: &str, root_path: &str, sep: char) -> Result<Index> {
    let f = std::fs::File::open(source).with_context(|| format!("opening {source}"))?;
    let vol = Volume::open(std::io::BufReader::with_capacity(1 << 20, f))?;
    vol.scan(root_path, sep)
}

/// Windows: scan drive letter like "C" through `\\.\C:` (requires admin).
pub fn scan_drive(letter: char) -> Result<Index> {
    let dev = format!("\\\\.\\{}:", letter.to_ascii_uppercase());
    let f = std::fs::File::open(&dev).with_context(|| format!("opening {dev} (run elevated)"))?;
    let vol = Volume::open(f)?;
    vol.scan(&format!("{}:\\", letter.to_ascii_uppercase()), '\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs() {
        // len 0x18 clusters at lcn 0x5634, then len 0x10 at -0x10 relative, then sparse 4.
        let buf = [0x21, 0x18, 0x34, 0x56, 0x11, 0x10, 0xF0, 0x01, 0x04, 0x00];
        let r = parse_runs(&buf);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].lcn, Some(0x5634));
        assert_eq!(r[1].lcn, Some(0x5634 - 0x10));
        assert_eq!(r[2].lcn, None);
        assert_eq!(r[2].clusters, 4);
    }

    #[test]
    fn attr_list_entries() {
        let mut v = vec![0u8; 0x40];
        v[0..4].copy_from_slice(&0x80u32.to_le_bytes());
        v[4..6].copy_from_slice(&0x20u16.to_le_bytes());
        v[8..16].copy_from_slice(&1234u64.to_le_bytes());
        v[0x10..0x18].copy_from_slice(&(17u64 | (3u64 << 48)).to_le_bytes());
        v[0x20..0x24].copy_from_slice(&0x30u32.to_le_bytes());
        v[0x24..0x26].copy_from_slice(&0x20u16.to_le_bytes());
        let l = parse_attr_list(&v);
        assert_eq!(l, vec![(0x80, 1234, 17), (0x30, 0, 0)]);
    }
}
