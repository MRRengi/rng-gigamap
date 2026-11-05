//! Compact text reports. Shared by the CLI and the MCP server, so agents get
//! the same few-hundred-token answers a human reads in the terminal.

use gigamap_core::cleanup::{self, Safety};
use gigamap_core::diff::Growth;
use gigamap_core::fmt::{datetime, delta, percent, size};
use gigamap_core::{query, Index, NodeId};
use std::fmt::Write;

pub fn overview(idx: &Index) -> String {
    let mut s = String::new();
    let total = idx.total_size[0];
    let _ = writeln!(s, "{}  {} in {} files", idx.root_path, size(total), idx.file_count[0]);
    if idx.volume_total > 0 {
        let used = idx.volume_total.saturating_sub(idx.volume_free);
        let _ = writeln!(
            s,
            "volume: {} used of {} ({} free)",
            size(used),
            size(idx.volume_total),
            size(idx.volume_free)
        );
        let unexplained = used.saturating_sub(idx.total_alloc[0]);
        if unexplained > idx.volume_total / 50 {
            let _ = writeln!(s, "not attributed to files: {} (shadow copies, restore points, unreadable folders)", size(unexplained));
        }
    }
    let _ = writeln!(s, "scanned {} via {:?} in {} ms", datetime(idx.scanned_at), idx.method.unwrap(), idx.scan_ms);
    s
}

pub fn breakdown(idx: &Index, n: NodeId, limit: usize) -> String {
    let b = query::breakdown(idx, n, limit);
    let total = b.node.size.max(1);
    let mut s = String::new();
    let _ = writeln!(s, "{}  {}", b.node.path, size(b.node.size));
    for it in &b.items {
        let _ = writeln!(
            s,
            "{:>9}  {:>5}  {}{}",
            size(it.size),
            percent(it.size, total),
            it.name,
            if it.is_dir { format!("\\  ({} files)", it.files) } else { String::new() }
        );
    }
    if b.rest_count > 0 {
        let _ = writeln!(s, "{:>9}  {:>5}  ...{} more", size(b.rest_size), percent(b.rest_size, total), b.rest_count);
    }
    s
}

pub fn top(idx: &Index, n: NodeId, limit: usize) -> String {
    let mut s = String::from("largest files:\n");
    for it in query::top_files(idx, n, limit, 0) {
        let _ = writeln!(s, "{:>9}  {}", size(it.size), it.path);
    }
    s.push_str("\nheaviest folders:\n");
    for it in query::hotspots(idx, n, limit) {
        let _ = writeln!(s, "{:>9}  {}  ({} files)", size(it.size), it.path, it.files);
    }
    s
}

pub fn types(idx: &Index, n: NodeId, limit: usize) -> String {
    let total = idx.total_size[n as usize].max(1);
    let mut s = String::from("by file type:\n");
    for t in query::file_types(idx, n, limit) {
        let ext = if t.ext.is_empty() { "(none)".to_string() } else { format!(".{}", t.ext) };
        let _ = writeln!(s, "{:>9}  {:>5}  {:<14} {} files", size(t.size), percent(t.size, total), ext, t.files);
    }
    s
}

pub fn cleanup(idx: &Index, n: NodeId, limit: usize) -> String {
    let all = cleanup::candidates(idx, n, 16 << 20);
    let safe: u64 = all.iter().filter(|c| c.safety == Safety::Safe).map(|c| c.size).sum();
    let mut s = format!("cleanup candidates ({} regenerable):\n", size(safe));
    for c in all.iter().take(limit) {
        let tag = match c.safety {
            Safety::Safe => "safe",
            Safety::Review => "review",
            Safety::System => "system",
        };
        let _ = writeln!(s, "{:>9}  [{tag}] {}  - {}: {}", size(c.size), c.path, c.kind, c.why);
    }
    if all.is_empty() {
        s.push_str("nothing over 16 MB found\n");
    }
    s
}

pub fn growth(g: &Growth) -> String {
    let mut s = format!(
        "since {} total changed {}\n",
        datetime(g.since),
        delta(g.total_delta)
    );
    if !g.grown.is_empty() {
        s.push_str("grew:\n");
        for c in &g.grown {
            let _ = writeln!(s, "{:>10}  {}{}  (now {})", delta(c.delta), c.path, if c.is_new { " [new]" } else { "" }, size(c.after));
        }
    }
    if !g.shrunk.is_empty() {
        s.push_str("shrank:\n");
        for c in &g.shrunk {
            let _ = writeln!(s, "{:>10}  {}", delta(c.delta), c.path);
        }
    }
    if !g.new_files.is_empty() {
        s.push_str("new or changed big files:\n");
        for c in &g.new_files {
            let _ = writeln!(s, "{:>10}  {}", size(c.after), c.path);
        }
    }
    s
}

pub fn summary_json(idx: &Index) -> serde_json::Value {
    let b = query::breakdown(idx, 0, 50);
    serde_json::json!({
        "root": idx.root_path,
        "total": idx.total_size[0],
        "alloc": idx.total_alloc[0],
        "files": idx.file_count[0],
        "items": idx.len(),
        "scan_ms": idx.scan_ms,
        "method": format!("{:?}", idx.method.unwrap()),
        "volume_total": idx.volume_total,
        "volume_free": idx.volume_free,
        "children": b.items,
    })
}

pub fn export_tsv(idx: &Index) {
    use std::io::Write as _;
    let out = std::io::stdout();
    let mut w = std::io::BufWriter::new(out.lock());
    for n in 0..idx.len() as NodeId {
        if idx.is_dir(n) {
            let _ = writeln!(w, "{}\t{}\t{}", idx.path(n), idx.total_size[n as usize], idx.file_count[n as usize]);
        }
    }
}
