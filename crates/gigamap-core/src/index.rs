//! Compact struct-of-arrays index of a volume or folder tree.
//! Node 0 is always the root. Names live in a single string pool.

use serde::{Deserialize, Serialize};

pub type NodeId = u32;
pub const NONE: NodeId = u32::MAX;

pub mod flag {
    pub const DIR: u8 = 1;
    pub const HIDDEN: u8 = 2;
    pub const SYSTEM: u8 = 4;
    pub const REPARSE: u8 = 8;
    /// NTFS metadata file ($MFT, $LogFile, ...).
    pub const META: u8 = 16;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanMethod {
    Mft,
    Walk,
}

#[derive(Default)]
pub struct Index {
    /// Root as the user sees it: "C:\" or "/data".
    pub root_path: String,
    pub sep: char,
    pub method: Option<ScanMethod>,
    pub scanned_at: i64,
    pub scan_ms: u64,
    pub volume_total: u64,
    pub volume_free: u64,

    pub parent: Vec<NodeId>,
    pub name_off: Vec<u32>,
    pub name_len: Vec<u16>,
    pub names: String,
    /// Own logical size (files; 0 for directories).
    pub size: Vec<u64>,
    /// Own allocated size on disk.
    pub alloc: Vec<u64>,
    /// Last modified, unix seconds.
    pub mtime: Vec<i64>,
    pub flags: Vec<u8>,

    // Filled by finalize().
    pub child_start: Vec<u32>,
    pub children: Vec<NodeId>,
    pub total_size: Vec<u64>,
    pub total_alloc: Vec<u64>,
    pub file_count: Vec<u32>,
}

impl Index {
    pub fn len(&self) -> usize {
        self.parent.len()
    }
    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }
    pub fn name(&self, n: NodeId) -> &str {
        let o = self.name_off[n as usize] as usize;
        &self.names[o..o + self.name_len[n as usize] as usize]
    }
    pub fn is_dir(&self, n: NodeId) -> bool {
        self.flags[n as usize] & flag::DIR != 0
    }
    pub fn kids(&self, n: NodeId) -> &[NodeId] {
        let s = self.child_start[n as usize] as usize;
        let e = self.child_start[n as usize + 1] as usize;
        &self.children[s..e]
    }

    pub fn path(&self, n: NodeId) -> String {
        if n == 0 {
            return self.root_path.clone();
        }
        let mut parts = Vec::new();
        let mut cur = n;
        let mut guard = 0;
        while cur != 0 && cur != NONE && guard < 4096 {
            parts.push(self.name(cur));
            cur = self.parent[cur as usize];
            guard += 1;
        }
        let mut s = self.root_path.clone();
        if !s.ends_with(self.sep) {
            s.push(self.sep);
        }
        for (i, p) in parts.iter().rev().enumerate() {
            if i > 0 {
                s.push(self.sep);
            }
            s.push_str(p);
        }
        s
    }

    /// Resolve a user path (absolute or relative to root), case-insensitive.
    pub fn find(&self, path: &str) -> Option<NodeId> {
        let root = self.root_path.trim_end_matches(['\\', '/']);
        let p = path.trim();
        let rel = if p.len() >= root.len() && p[..root.len()].eq_ignore_ascii_case(root) {
            &p[root.len()..]
        } else {
            p
        };
        let mut cur: NodeId = 0;
        for part in rel.split(['\\', '/']).filter(|s| !s.is_empty() && *s != ".") {
            cur = *self
                .kids(cur)
                .iter()
                .find(|&&k| self.name(k).eq_ignore_ascii_case(part))?;
        }
        Some(cur)
    }

    /// Depth-first iteration over a subtree (including the node itself).
    pub fn subtree(&self, n: NodeId) -> SubtreeIter<'_> {
        SubtreeIter { idx: self, stack: vec![n] }
    }

    pub fn is_ancestor(&self, anc: NodeId, mut n: NodeId) -> bool {
        let mut guard = 0;
        while n != NONE && guard < 4096 {
            if n == anc {
                return true;
            }
            if n == 0 {
                return false;
            }
            n = self.parent[n as usize];
            guard += 1;
        }
        false
    }

    pub fn extension(&self, n: NodeId) -> &str {
        let name = self.name(n);
        match name.rfind('.') {
            Some(i) if i > 0 && i + 1 < name.len() && name.len() - i <= 16 => &name[i + 1..],
            _ => "",
        }
    }

    /// Build CSR children, re-home unreachable nodes, aggregate subtree totals,
    /// sort children by size (largest first).
    pub fn finalize(&mut self) {
        let n = self.len();
        if n == 0 {
            return;
        }
        self.parent[0] = NONE;
        for i in 1..n {
            let p = self.parent[i];
            if p == NONE || p as usize >= n || p as usize == i || !self.is_dir(p) {
                self.parent[i] = 0;
            }
        }
        self.build_csr();
        // Re-home anything not reachable from the root (cycles from corrupt metadata).
        let mut seen = vec![false; n];
        let mut order = Vec::with_capacity(n);
        order.push(0u32);
        seen[0] = true;
        let mut head = 0;
        while head < order.len() {
            let cur = order[head];
            head += 1;
            for &k in self.kids(cur) {
                if !seen[k as usize] {
                    seen[k as usize] = true;
                    order.push(k);
                }
            }
        }
        if order.len() < n {
            for (i, s) in seen.iter().enumerate() {
                if !s {
                    self.parent[i] = 0;
                }
            }
            self.build_csr();
            return self.finalize_totals_after_csr();
        }
        self.aggregate(&order);
        self.sort_children();
    }

    fn finalize_totals_after_csr(&mut self) {
        let n = self.len();
        let mut order = Vec::with_capacity(n);
        order.push(0u32);
        let mut head = 0;
        while head < order.len() {
            let cur = order[head];
            head += 1;
            order.extend_from_slice(self.kids(cur));
        }
        self.aggregate(&order);
        self.sort_children();
    }

    fn build_csr(&mut self) {
        let n = self.len();
        let mut counts = vec![0u32; n + 1];
        for i in 1..n {
            counts[self.parent[i] as usize] += 1;
        }
        let mut start = vec![0u32; n + 1];
        let mut acc = 0u32;
        for i in 0..n {
            start[i] = acc;
            acc += counts[i];
        }
        start[n] = acc;
        let mut fill = start.clone();
        let mut children = vec![0u32; acc as usize];
        for i in 1..n {
            let p = self.parent[i] as usize;
            children[fill[p] as usize] = i as u32;
            fill[p] += 1;
        }
        self.child_start = start;
        self.children = children;
    }

    fn aggregate(&mut self, bfs_order: &[NodeId]) {
        let n = self.len();
        self.total_size = self.size.clone();
        self.total_alloc = self.alloc.clone();
        self.file_count = (0..n).map(|i| u32::from(!self.is_dir(i as u32))).collect();
        for &i in bfs_order.iter().rev() {
            let p = self.parent[i as usize];
            if p != NONE {
                let (s, a, c) = (
                    self.total_size[i as usize],
                    self.total_alloc[i as usize],
                    self.file_count[i as usize],
                );
                self.total_size[p as usize] += s;
                self.total_alloc[p as usize] += a;
                self.file_count[p as usize] += c;
            }
        }
    }

    fn sort_children(&mut self) {
        let n = self.len();
        let totals = &self.total_size;
        for i in 0..n {
            let s = self.child_start[i] as usize;
            let e = self.child_start[i + 1] as usize;
            if e - s > 1 {
                self.children[s..e].sort_unstable_by(|a, b| totals[*b as usize].cmp(&totals[*a as usize]));
            }
        }
    }
}

pub struct SubtreeIter<'a> {
    idx: &'a Index,
    stack: Vec<NodeId>,
}

impl Iterator for SubtreeIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let n = self.stack.pop()?;
        self.stack.extend_from_slice(self.idx.kids(n));
        Some(n)
    }
}

/// Incremental builder used by both scanners.
pub struct IndexBuilder {
    pub idx: Index,
}

impl IndexBuilder {
    pub fn new(root_path: &str, sep: char, method: ScanMethod) -> Self {
        let mut idx = Index {
            root_path: root_path.to_string(),
            sep,
            method: Some(method),
            ..Default::default()
        };
        idx.scanned_at = now_unix();
        let mut b = IndexBuilder { idx };
        b.push(NONE, "", 0, 0, 0, flag::DIR);
        b
    }

    pub fn reserve(&mut self, n: usize, name_bytes: usize) {
        let i = &mut self.idx;
        i.parent.reserve(n);
        i.name_off.reserve(n);
        i.name_len.reserve(n);
        i.size.reserve(n);
        i.alloc.reserve(n);
        i.mtime.reserve(n);
        i.flags.reserve(n);
        i.names.reserve(name_bytes);
    }

    pub fn push(&mut self, parent: NodeId, name: &str, size: u64, alloc: u64, mtime: i64, flags: u8) -> NodeId {
        let i = &mut self.idx;
        let id = i.parent.len() as NodeId;
        let name = truncate_name(name);
        i.parent.push(parent);
        i.name_off.push(i.names.len() as u32);
        i.name_len.push(name.len() as u16);
        i.names.push_str(name);
        i.size.push(size);
        i.alloc.push(alloc);
        i.mtime.push(mtime);
        i.flags.push(flags);
        id
    }

    pub fn finish(mut self, started: std::time::Instant) -> Index {
        self.idx.finalize();
        self.idx.scan_ms = started.elapsed().as_millis() as u64;
        self.idx
    }
}

fn truncate_name(s: &str) -> &str {
    if s.len() <= u16::MAX as usize {
        return s;
    }
    let mut end = u16::MAX as usize;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> Index {
        let mut b = IndexBuilder::new("C:\\", '\\', ScanMethod::Walk);
        let users = b.push(0, "Users", 0, 0, 0, flag::DIR);
        let maks = b.push(users, "Maks", 0, 0, 0, flag::DIR);
        b.push(maks, "video.mp4", 1000, 1024, 0, 0);
        b.push(maks, "notes.txt", 10, 4096, 0, 0);
        let win = b.push(0, "Windows", 0, 0, 0, flag::DIR);
        b.push(win, "big.dll", 5000, 8192, 0, 0);
        b.finish(std::time::Instant::now())
    }

    #[test]
    fn totals_and_paths() {
        let i = sample();
        assert_eq!(i.total_size[0], 6010);
        assert_eq!(i.file_count[0], 3);
        assert_eq!(i.name(i.kids(0)[0]), "Windows");
        let v = i.find("C:\\users\\maks\\VIDEO.mp4").unwrap();
        assert_eq!(i.path(v), "C:\\Users\\Maks\\video.mp4");
        assert_eq!(i.extension(v), "mp4");
        assert_eq!(i.subtree(0).count(), 7);
    }
}
