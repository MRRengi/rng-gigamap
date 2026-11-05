//! Read queries over an Index. Everything returns small, pre-digested results.

use crate::index::{Index, NodeId};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

#[derive(Serialize, Clone, Debug)]
pub struct Item {
    pub id: NodeId,
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub alloc: u64,
    pub files: u32,
    pub mtime: i64,
}

impl Index {
    pub fn item(&self, n: NodeId) -> Item {
        Item {
            id: n,
            path: self.path(n),
            name: self.name(n).to_string(),
            is_dir: self.is_dir(n),
            size: self.total_size[n as usize],
            alloc: self.total_alloc[n as usize],
            files: self.file_count[n as usize],
            mtime: self.mtime[n as usize],
        }
    }
}

/// Direct children of a folder, largest first. Remaining ones are summed in `rest`.
pub struct Breakdown {
    pub node: Item,
    pub items: Vec<Item>,
    pub rest_count: usize,
    pub rest_size: u64,
}

pub fn breakdown(idx: &Index, n: NodeId, limit: usize) -> Breakdown {
    let kids = idx.kids(n);
    let items = kids.iter().take(limit).map(|&k| idx.item(k)).collect();
    let rest: &[NodeId] = if kids.len() > limit { &kids[limit..] } else { &[] };
    Breakdown {
        node: idx.item(n),
        items,
        rest_count: rest.len(),
        rest_size: rest.iter().map(|&k| idx.total_size[k as usize]).sum(),
    }
}

/// Largest files under a node.
pub fn top_files(idx: &Index, n: NodeId, limit: usize, min_size: u64) -> Vec<Item> {
    let mut heap: BinaryHeap<Reverse<(u64, NodeId)>> = BinaryHeap::with_capacity(limit + 1);
    let mut consider = |k: NodeId| {
        let s = idx.size[k as usize];
        if s >= min_size && !idx.is_dir(k) {
            if heap.len() < limit {
                heap.push(Reverse((s, k)));
            } else if heap.peek().is_some_and(|Reverse((m, _))| s > *m) {
                heap.pop();
                heap.push(Reverse((s, k)));
            }
        }
    };
    if n == 0 {
        (0..idx.len() as NodeId).for_each(&mut consider);
    } else {
        idx.subtree(n).for_each(&mut consider);
    }
    let mut v: Vec<_> = heap.into_iter().map(|Reverse((_, k))| k).collect();
    v.sort_unstable_by(|a, b| idx.size[*b as usize].cmp(&idx.size[*a as usize]));
    v.into_iter().map(|k| idx.item(k)).collect()
}

/// "Hotspot" folders: the biggest folders that are not just a wrapper around one
/// child folder, picked so that no two overlap (their sizes add up honestly).
pub fn hotspots(idx: &Index, n: NodeId, limit: usize) -> Vec<Item> {
    let floor = (idx.total_size[n as usize] / 500).max(1);
    let mut cands: Vec<NodeId> = idx
        .subtree(n)
        .filter(|&k| k != n && idx.is_dir(k) && idx.total_size[k as usize] >= floor)
        .filter(|&k| {
            let total = idx.total_size[k as usize];
            let biggest = idx.kids(k).iter().find(|&&c| idx.is_dir(c)).map_or(0, |&c| idx.total_size[c as usize]);
            biggest * 10 < total * 7
        })
        .collect();
    cands.sort_unstable_by(|a, b| idx.total_size[*b as usize].cmp(&idx.total_size[*a as usize]));
    let mut chosen: Vec<NodeId> = Vec::new();
    for c in cands {
        if chosen.len() >= limit {
            break;
        }
        if !chosen.iter().any(|&x| idx.is_ancestor(x, c) || idx.is_ancestor(c, x)) {
            chosen.push(c);
        }
    }
    chosen.into_iter().map(|k| idx.item(k)).collect()
}

#[derive(Serialize, Clone, Debug)]
pub struct TypeStat {
    pub ext: String,
    pub size: u64,
    pub files: u64,
}

pub fn file_types(idx: &Index, n: NodeId, limit: usize) -> Vec<TypeStat> {
    let mut m: HashMap<String, (u64, u64)> = HashMap::new();
    let mut add = |k: NodeId| {
        if !idx.is_dir(k) {
            let e = idx.extension(k).to_ascii_lowercase();
            let s = m.entry(e).or_default();
            s.0 += idx.size[k as usize];
            s.1 += 1;
        }
    };
    if n == 0 {
        (0..idx.len() as NodeId).for_each(&mut add);
    } else {
        idx.subtree(n).for_each(&mut add);
    }
    let mut v: Vec<TypeStat> = m
        .into_iter()
        .map(|(ext, (size, files))| TypeStat { ext, size, files })
        .collect();
    v.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    v.truncate(limit);
    v
}

/// Case-insensitive glob (`*`, `?`) over names, largest matches first.
pub fn search(idx: &Index, n: NodeId, pattern: &str, limit: usize) -> Vec<Item> {
    let pat: Vec<char> = pattern.to_lowercase().chars().collect();
    let has_wild = pat.iter().any(|c| *c == '*' || *c == '?');
    let mut hits: Vec<NodeId> = idx
        .subtree(n)
        .filter(|&k| k != 0)
        .filter(|&k| {
            let name = idx.name(k).to_lowercase();
            if has_wild {
                glob(&pat, &name.chars().collect::<Vec<_>>())
            } else {
                name.contains(&pattern.to_lowercase())
            }
        })
        .collect();
    hits.sort_unstable_by(|a, b| idx.total_size[*b as usize].cmp(&idx.total_size[*a as usize]));
    hits.truncate(limit);
    hits.into_iter().map(|k| idx.item(k)).collect()
}

fn glob(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{flag, IndexBuilder, ScanMethod};

    fn idx() -> Index {
        let mut b = IndexBuilder::new("C:\\", '\\', ScanMethod::Walk);
        let u = b.push(0, "Users", 0, 0, 0, flag::DIR);
        let m = b.push(u, "Maks", 0, 0, 0, flag::DIR);
        let p = b.push(m, "Projects", 0, 0, 0, flag::DIR);
        let a = b.push(p, "GameA", 0, 0, 0, flag::DIR);
        let g = b.push(p, "GameB", 0, 0, 0, flag::DIR);
        b.push(a, "big.unitypackage", 6000, 6000, 0, 0);
        b.push(g, "clip.mp4", 4000, 4000, 0, 0);
        b.push(0, "pagefile.sys", 100, 100, 0, 0);
        b.finish(std::time::Instant::now())
    }

    #[test]
    fn hotspots_skip_single_chains() {
        let i = idx();
        let h = hotspots(&i, 0, 5);
        // Users -> Maks -> Projects is a single chain; Projects splits 60/40.
        assert_eq!(h[0].name, "Projects");
    }

    #[test]
    fn top_and_types_and_search() {
        let i = idx();
        assert_eq!(top_files(&i, 0, 2, 0)[0].name, "big.unitypackage");
        assert_eq!(file_types(&i, 0, 5)[0].ext, "unitypackage");
        assert_eq!(search(&i, 0, "*.mp4", 5)[0].name, "clip.mp4");
        assert!(glob(&"a*c?".chars().collect::<Vec<_>>(), &"abbbcd".chars().collect::<Vec<_>>()));
    }
}
