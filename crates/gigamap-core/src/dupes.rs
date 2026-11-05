//! Duplicate files: group by size (free, from the index), then hash only the
//! groups that collide - first+last 64 KB, then the full file.

use crate::index::{Index, NodeId};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

#[derive(Serialize, Clone, Debug)]
pub struct DupeGroup {
    pub size: u64,
    pub paths: Vec<String>,
    /// Space freed by keeping one copy.
    pub wasted: u64,
}

const EDGE: usize = 64 * 1024;

fn edge_hash(path: &str, size: u64) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; EDGE.min(size as usize)];
    f.read_exact(&mut buf).ok()?;
    h.update(&buf);
    if size as usize > EDGE * 2 {
        f.seek(SeekFrom::End(-(EDGE as i64))).ok()?;
        f.read_exact(&mut buf).ok()?;
        h.update(&buf);
    }
    Some(*h.finalize().as_bytes())
}

fn full_hash(path: &str) -> Option<[u8; 32]> {
    let f = std::fs::File::open(path).ok()?;
    let mut h = blake3::Hasher::new();
    h.update_reader(std::io::BufReader::with_capacity(1 << 20, f)).ok()?;
    Some(*h.finalize().as_bytes())
}

pub fn find(idx: &Index, scope: NodeId, min_size: u64, limit: usize) -> Vec<DupeGroup> {
    let mut by_size: HashMap<u64, Vec<NodeId>> = HashMap::new();
    let nodes: Box<dyn Iterator<Item = NodeId>> = if scope == 0 {
        Box::new(0..idx.len() as NodeId)
    } else {
        Box::new(idx.subtree(scope))
    };
    for n in nodes {
        let s = idx.size[n as usize];
        if !idx.is_dir(n) && s >= min_size && idx.flags[n as usize] & crate::index::flag::META == 0 {
            by_size.entry(s).or_default().push(n);
        }
    }
    let mut groups: Vec<(u64, Vec<String>)> = by_size
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(s, v)| (s, v.into_iter().map(|n| idx.path(n)).collect()))
        .collect();
    // Biggest potential waste first, so a limited run finds what matters.
    groups.sort_unstable_by_key(|(s, v)| std::cmp::Reverse(s * (v.len() as u64 - 1)));

    let mut out: Vec<DupeGroup> = groups
        .par_iter()
        .flat_map_iter(|(size, paths)| {
            let mut by_edge: HashMap<[u8; 32], Vec<&String>> = HashMap::new();
            for p in paths {
                if let Some(h) = edge_hash(p, *size) {
                    by_edge.entry(h).or_default().push(p);
                }
            }
            let mut res = Vec::new();
            for (_, cand) in by_edge.into_iter().filter(|(_, v)| v.len() > 1) {
                let mut by_full: HashMap<[u8; 32], Vec<String>> = HashMap::new();
                for p in cand {
                    let h = if *size as usize <= EDGE * 2 { edge_hash(p, *size) } else { full_hash(p) };
                    if let Some(h) = h {
                        by_full.entry(h).or_default().push(p.clone());
                    }
                }
                for (_, same) in by_full.into_iter().filter(|(_, v)| v.len() > 1) {
                    res.push(DupeGroup { size: *size, wasted: size * (same.len() as u64 - 1), paths: same });
                }
            }
            res
        })
        .collect();
    out.sort_unstable_by_key(|g| std::cmp::Reverse(g.wasted));
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_identical_files() {
        let d = std::env::temp_dir().join(format!("gm-dup-{}", std::process::id()));
        std::fs::create_dir_all(d.join("x")).unwrap();
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(d.join("a.bin"), &data).unwrap();
        std::fs::write(d.join("x/b.bin"), &data).unwrap();
        let mut other = data.clone();
        other[150_000] ^= 1; // same size, same edges, different middle
        std::fs::write(d.join("x/c.bin"), &other).unwrap();
        let i = crate::walk::scan(&d).unwrap();
        let g = super::find(&i, 0, 1, 10);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].paths.len(), 2);
        std::fs::remove_dir_all(&d).ok();
    }
}
