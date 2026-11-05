//! What changed between two scans of the same root.

use crate::index::{Index, NodeId};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Clone, Debug)]
pub struct Change {
    pub path: String,
    pub before: u64,
    pub after: u64,
    pub delta: i64,
    pub is_new: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Growth {
    pub since: i64,
    pub now: i64,
    pub total_delta: i64,
    /// Folders that grew, most specific useful level first by size of growth.
    pub grown: Vec<Change>,
    /// Folders that shrank.
    pub shrunk: Vec<Change>,
    /// Big files modified after the previous scan.
    pub new_files: Vec<Change>,
}

fn key(idx: &Index, n: NodeId) -> String {
    idx.path(n).to_lowercase()
}

pub fn growth(old: &Index, new: &Index, limit: usize) -> Growth {
    let mut old_dirs: HashMap<String, u64> = HashMap::with_capacity(old.len() / 8);
    for n in 0..old.len() as NodeId {
        if old.is_dir(n) {
            old_dirs.insert(key(old, n), old.total_size[n as usize]);
        }
    }
    let delta_of = |n: NodeId| -> (i64, u64, bool) {
        let after = new.total_size[n as usize];
        match old_dirs.get(&key(new, n)) {
            Some(&b) => (after as i64 - b as i64, b, false),
            None => (after as i64, 0, true),
        }
    };
    let total_delta = new.total_size[0] as i64 - old.total_size[0] as i64;

    // Walk down while one child folder explains >= 70% of the parent's growth.
    let mut grown: Vec<(NodeId, i64, u64, bool)> = Vec::new();
    let mut shrunk: Vec<(NodeId, i64, u64, bool)> = Vec::new();
    let threshold = (new.total_size[0] / 2000).max(1 << 20) as i64; // >= 1 MB and >= 0.05%
    let mut stack: Vec<NodeId> = vec![0];
    while let Some(cur) = stack.pop() {
        let (d, before, is_new) = delta_of(cur);
        if d.abs() < threshold {
            continue;
        }
        let mut kid_deltas: Vec<(NodeId, i64)> = new
            .kids(cur)
            .iter()
            .filter(|&&k| new.is_dir(k))
            .map(|&k| (k, delta_of(k).0))
            .filter(|(_, kd)| kd.signum() == d.signum() && kd.abs() >= threshold)
            .collect();
        kid_deltas.sort_unstable_by_key(|(_, kd)| std::cmp::Reverse(kd.abs()));
        let dominant = kid_deltas.first().is_some_and(|(_, kd)| kd.abs() * 10 >= d.abs() * 7);
        if !dominant && cur != 0 {
            if d > 0 {
                grown.push((cur, d, before, is_new));
            } else {
                shrunk.push((cur, d, before, is_new));
            }
        }
        if !is_new {
            stack.extend(kid_deltas.into_iter().map(|(k, _)| k));
        }
    }
    let to_change = |v: Vec<(NodeId, i64, u64, bool)>| -> Vec<Change> {
        let mut v = v;
        v.sort_unstable_by_key(|x| std::cmp::Reverse(x.1.abs()));
        v.truncate(limit);
        v.into_iter()
            .map(|(n, d, before, is_new)| Change {
                path: new.path(n),
                before,
                after: new.total_size[n as usize],
                delta: d,
                is_new,
            })
            .collect()
    };

    let mut files: Vec<NodeId> = (0..new.len() as NodeId)
        .filter(|&n| !new.is_dir(n) && new.mtime[n as usize] > old.scanned_at && new.size[n as usize] >= 1 << 20)
        .collect();
    files.sort_unstable_by_key(|&n| std::cmp::Reverse(new.size[n as usize]));
    files.truncate(limit);
    let new_files = files
        .into_iter()
        .map(|n| Change {
            path: new.path(n),
            before: 0,
            after: new.size[n as usize],
            delta: new.size[n as usize] as i64,
            is_new: true,
        })
        .collect();

    Growth {
        since: old.scanned_at,
        now: new.scanned_at,
        total_delta,
        grown: to_change(grown),
        shrunk: to_change(shrunk),
        new_files,
    }
}

#[cfg(test)]
mod tests {
    use crate::index::{flag, IndexBuilder, ScanMethod};

    #[test]
    fn finds_grown_leaf_folder() {
        let mk = |cache: u64| {
            let mut b = IndexBuilder::new("C:\\", '\\', ScanMethod::Walk);
            let u = b.push(0, "Users", 0, 0, 0, flag::DIR);
            let c = b.push(u, "Cache", 0, 0, 0, flag::DIR);
            b.push(c, "blob", cache, cache, 0, 0);
            b.push(u, "doc", 10 << 20, 0, 0, 0);
            b.finish(std::time::Instant::now())
        };
        let g = super::growth(&mk(5 << 20), &mk(500 << 20), 5);
        assert_eq!(g.grown[0].path, "C:\\Users\\Cache");
        assert_eq!(g.grown[0].delta, (495i64) << 20);
    }
}
