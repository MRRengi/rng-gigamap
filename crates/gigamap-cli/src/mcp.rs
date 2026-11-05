//! MCP server over stdio (newline-delimited JSON-RPC 2.0).
//! Every tool returns a short text answer, so an agent learns what fills the
//! disk in a few hundred tokens instead of crawling folders itself.

use crate::{report, source};
use anyhow::{anyhow, Result};
use gigamap_core::{diff, dupes, fmt, query, requests, snapshot, Index, NodeId};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const STALE_SECS: i64 = 15 * 60;

struct State {
    cache: Vec<Index>,
}

pub fn serve() -> Result<()> {
    let mut st = State { cache: Vec::new() };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                reply(&mut out, json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":e.to_string()}}))?;
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned() else { continue }; // notification
        let method = msg["method"].as_str().unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let resp = match method {
            "initialize" => json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "gigamap", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Gigamap knows what takes space on this PC. Start with disk_overview, drill down with folder_breakdown, use top_items for the biggest files and hotspots, growth for what grew since the last scan, cleanup_candidates for regenerable junk. Deleting is only possible through request_delete, which the user must confirm in the Gigamap window; files go to the Recycle Bin."
            }),
            "ping" => json!({}),
            "tools/list" => json!({"tools": tools()}),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match st.call(name, &args) {
                    Ok(text) => json!({"content":[{"type":"text","text":text}]}),
                    Err(e) => json!({"content":[{"type":"text","text":format!("error: {e:#}")}],"isError":true}),
                }
            }
            _ => {
                reply(&mut out, json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("unknown method {method}")}}))?;
                continue;
            }
        };
        reply(&mut out, json!({"jsonrpc":"2.0","id":id,"result":resp}))?;
    }
    Ok(())
}

fn reply(out: &mut impl Write, v: Value) -> Result<()> {
    writeln!(out, "{v}")?;
    out.flush()?;
    Ok(())
}

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({"name": name, "description": desc, "inputSchema": {"type":"object","properties": props, "required": required}})
}

fn path_prop() -> Value {
    json!({"type":"string","description":"Drive (C:) or folder. Defaults to the system drive."})
}

fn tools() -> Vec<Value> {
    vec![
        tool("disk_overview", "Used/free space of a drive and its top-level breakdown. Start here.", json!({"path": path_prop()}), &[]),
        tool("folder_breakdown", "Direct children of a folder, largest first, with share of the parent.", json!({"path": path_prop(), "limit": {"type":"integer","default":15}}), &["path"]),
        tool("top_items", "Largest files and heaviest folders (hotspots, not every ancestor) under a path.", json!({"path": path_prop(), "limit": {"type":"integer","default":15}}), &[]),
        tool("file_types", "Space by file extension under a path.", json!({"path": path_prop(), "limit": {"type":"integer","default":15}}), &[]),
        tool("growth", "What grew or shrank since an earlier scan, plus big files changed since then.", json!({"path": path_prop(), "since": {"type":"string","description":"'last' (default) or a date YYYY-MM-DD"}}), &[]),
        tool("cleanup_candidates", "Regenerable build output, caches and temp folders (node_modules, Unity Library, cargo target, browser caches...) with a safety level.", json!({"path": path_prop(), "limit": {"type":"integer","default":20}}), &[]),
        tool("find_duplicates", "Identical files (size, then content hash). Reads file contents, can take a while on big scopes.", json!({"path": path_prop(), "min_size_mb": {"type":"integer","default":50}, "limit": {"type":"integer","default":10}}), &[]),
        tool("search", "Find files or folders by name (glob like *.mp4 or plain substring), largest first.", json!({"pattern": {"type":"string"}, "path": path_prop(), "limit": {"type":"integer","default":20}}), &["pattern"]),
        tool("rescan", "Rescan a drive or folder now (results are otherwise cached for 15 minutes).", json!({"path": path_prop()}), &[]),
        tool("request_delete", "Ask the user to move paths to the Recycle Bin. The user confirms or declines in the Gigamap window; nothing happens without that. Returns a request id.", json!({"paths": {"type":"array","items":{"type":"string"}}, "reason": {"type":"string","description":"One line the user will read."}}), &["paths","reason"]),
        tool("delete_status", "State of a delete request: pending, done, declined or failed.", json!({"id": {"type":"string"}}), &["id"]),
    ]
}

fn arg_usize(a: &Value, k: &str, d: usize) -> usize {
    a.get(k).and_then(Value::as_u64).map_or(d, |v| v.clamp(1, 200) as usize)
}

impl State {
    /// Scan target for a path: its drive on Windows, otherwise the path itself.
    fn target_for(path: &str) -> String {
        if cfg!(windows) {
            if let Some(l) = path.chars().next().filter(|c| c.is_ascii_alphabetic() && path[1..].starts_with(':')) {
                return format!("{l}:");
            }
        }
        path.to_string()
    }

    fn index_for(&mut self, path: Option<&str>, force: bool) -> Result<(usize, NodeId)> {
        let path = path.map(str::to_string).unwrap_or_else(source::default_target);
        let now = gigamap_core::index::now_unix();
        if !force {
            for (i, idx) in self.cache.iter().enumerate() {
                if now - idx.scanned_at < STALE_SECS {
                    if let Some(n) = idx.find(&path) {
                        return Ok((i, n));
                    }
                }
            }
        }
        let target = Self::target_for(&path);
        let idx = source::scan(&target, None)?;
        snapshot::store(&snapshot::default_base(), &idx, 10).ok();
        self.cache.retain(|c| !c.root_path.eq_ignore_ascii_case(&idx.root_path));
        self.cache.push(idx);
        let i = self.cache.len() - 1;
        let n = self.cache[i].find(&path).ok_or_else(|| anyhow!("{path} not found after scan"))?;
        Ok((i, n))
    }

    fn call(&mut self, name: &str, a: &Value) -> Result<String> {
        let path = a.get("path").and_then(Value::as_str);
        match name {
            "disk_overview" => {
                let (i, n) = self.index_for(path, false)?;
                let idx = &self.cache[i];
                Ok(format!("{}{}", report::overview(idx), report::breakdown(idx, n, 12)))
            }
            "folder_breakdown" => {
                let (i, n) = self.index_for(path, false)?;
                Ok(report::breakdown(&self.cache[i], n, arg_usize(a, "limit", 15)))
            }
            "top_items" => {
                let (i, n) = self.index_for(path, false)?;
                Ok(report::top(&self.cache[i], n, arg_usize(a, "limit", 15)))
            }
            "file_types" => {
                let (i, n) = self.index_for(path, false)?;
                Ok(report::types(&self.cache[i], n, arg_usize(a, "limit", 15)))
            }
            "cleanup_candidates" => {
                let (i, n) = self.index_for(path, false)?;
                Ok(report::cleanup(&self.cache[i], n, arg_usize(a, "limit", 20)))
            }
            "growth" => {
                let (i, _) = self.index_for(path, false)?;
                let cur = &self.cache[i];
                let dir = snapshot::dir_for(&snapshot::default_base(), &cur.root_path);
                let snaps = snapshot::list(&dir);
                let since = a.get("since").and_then(Value::as_str).unwrap_or("last");
                let pick = if since == "last" {
                    snaps.iter().rev().find(|(ts, _)| *ts < cur.scanned_at)
                } else {
                    let want = parse_date(since).ok_or_else(|| anyhow!("since must be 'last' or YYYY-MM-DD"))?;
                    snaps.iter().rev().find(|(ts, _)| *ts <= want).or_else(|| snaps.first())
                };
                let Some((_, p)) = pick else {
                    return Ok(format!(
                        "no earlier scan of {} yet; this scan is now saved, ask again after the next scan",
                        cur.root_path
                    ));
                };
                let old = snapshot::load(p)?;
                Ok(report::growth(&diff::growth(&old, cur, 15)))
            }
            "find_duplicates" => {
                let (i, n) = self.index_for(path, false)?;
                let min = a.get("min_size_mb").and_then(Value::as_u64).unwrap_or(50) << 20;
                let groups = dupes::find(&self.cache[i], n, min, arg_usize(a, "limit", 10));
                if groups.is_empty() {
                    return Ok(format!("no duplicates of {} or more", fmt::size(min)));
                }
                let wasted: u64 = groups.iter().map(|g| g.wasted).sum();
                let mut s = format!("duplicates ({} reclaimable by keeping one copy):\n", fmt::size(wasted));
                for g in groups {
                    s.push_str(&format!("{} x {}:\n", g.paths.len(), fmt::size(g.size)));
                    for p in g.paths {
                        s.push_str(&format!("    {p}\n"));
                    }
                }
                Ok(s)
            }
            "search" => {
                let pat = a.get("pattern").and_then(Value::as_str).ok_or_else(|| anyhow!("pattern is required"))?;
                let (i, n) = self.index_for(path, false)?;
                let hits = query::search(&self.cache[i], n, pat, arg_usize(a, "limit", 20));
                if hits.is_empty() {
                    return Ok(format!("nothing matches {pat}"));
                }
                Ok(hits
                    .iter()
                    .map(|h| format!("{:>9}  {}{}\n", fmt::size(h.size), h.path, if h.is_dir { "\\" } else { "" }))
                    .collect())
            }
            "rescan" => {
                let (i, _) = self.index_for(path, true)?;
                Ok(report::overview(&self.cache[i]))
            }
            "request_delete" => {
                let paths: Vec<String> = a
                    .get("paths")
                    .and_then(Value::as_array)
                    .map(|v| v.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                anyhow::ensure!(!paths.is_empty(), "paths is empty");
                let reason = a.get("reason").and_then(Value::as_str).unwrap_or("");
                let sizes: Vec<u64> = paths
                    .iter()
                    .map(|p| self.cache.iter().find_map(|c| c.find(p).map(|n| c.total_size[n as usize])).unwrap_or(0))
                    .collect();
                let total: u64 = sizes.iter().sum();
                let r = requests::submit(paths, sizes, reason)?;
                Ok(format!(
                    "request {} sent: {} items, {}. The user confirms it in the Gigamap window; check with delete_status.",
                    r.id,
                    r.paths.len(),
                    fmt::size(total)
                ))
            }
            "delete_status" => {
                let id = a.get("id").and_then(Value::as_str).ok_or_else(|| anyhow!("id is required"))?;
                let r = requests::get(id)?;
                Ok(format!("{:?}{}", r.state, if r.message.is_empty() { String::new() } else { format!(": {}", r.message) }).to_lowercase())
            }
            other => Err(anyhow!("unknown tool {other}")),
        }
    }
}

fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    // days from civil (inverse of fmt::datetime)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400 + 86_399)
}
