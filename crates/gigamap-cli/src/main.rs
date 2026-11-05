mod mcp;
mod report;
mod service;
mod source;

use anyhow::{bail, Result};
use gigamap_core::fmt;

const HELP: &str = "Gigamap - ultra-fast disk usage analyzer

USAGE:
  gigamap scan <target> [--image FILE] [--save] [--json]
  gigamap top <target> [--image FILE] [-n N]
  gigamap export <target> [--image FILE]      tab-separated: path, size, files (folders only)
  gigamap mcp                                 MCP server on stdio (for coding agents)
  gigamap service install|uninstall|--console  elevated helper that keeps scans UAC-free
  gigamap selfcheck <folder>                  compare MFT scan with a folder walk

<target> is a drive (C:) or a folder. Drives are read straight from the NTFS
master file table when running elevated; otherwise folders are walked in parallel.
--image scans an NTFS image file and labels it as <target>.";

/// Compare the drive's MFT scan with a folder walk of `folder` (same data, two methods).
fn selfcheck(folder: Option<&str>) -> Result<()> {
    let folder = folder.ok_or_else(|| anyhow::anyhow!("usage: gigamap selfcheck <folder>"))?;
    let letter = source::drive_letter(&folder[..2.min(folder.len())])
        .ok_or_else(|| anyhow::anyhow!("selfcheck needs a folder on a drive, like C:\\data"))?;
    let mft = gigamap_core::mft::scan_drive(letter)?;
    eprintln!("MFT: {} items in {} ms", mft.len(), mft.scan_ms);
    let walk = gigamap_core::walk::scan(std::path::Path::new(folder))?;
    eprintln!("walk: {} items in {} ms", walk.len(), walk.scan_ms);
    let base = mft.find(folder).ok_or_else(|| anyhow::anyhow!("{folder} not in MFT scan"))?;
    let mut bad = 0;
    let mut checked = 0;
    for w in 0..walk.len() as u32 {
        if !walk.is_dir(w) {
            continue;
        }
        let p = walk.path(w);
        let rel = p[walk.root_path.len()..].trim_start_matches('\\');
        let m = if rel.is_empty() { Some(base) } else { mft.find(&format!("{}\\{}", mft.path(base), rel)) };
        checked += 1;
        match m {
            Some(m) if mft.total_size[m as usize] == walk.total_size[w as usize]
                && mft.file_count[m as usize] == walk.file_count[w as usize] => {}
            other => {
                bad += 1;
                if bad <= 20 {
                    eprintln!(
                        "MISMATCH {p}: walk {} B / {} files, mft {:?}",
                        walk.total_size[w as usize],
                        walk.file_count[w as usize],
                        other.map(|m| (mft.total_size[m as usize], mft.file_count[m as usize]))
                    );
                }
            }
        }
    }
    println!("{checked} folders compared, {bad} mismatches");
    if bad > 0 {
        bail!("selfcheck failed");
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

struct Args {
    cmd: String,
    target: Option<String>,
    image: Option<String>,
    n: usize,
    save: bool,
    json: bool,
}

fn parse() -> Result<Args> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_default();
    let mut a = Args { cmd, target: None, image: None, n: 20, save: false, json: false };
    while let Some(x) = it.next() {
        match x.as_str() {
            "--image" => a.image = it.next(),
            "-n" => a.n = it.next().and_then(|v| v.parse().ok()).unwrap_or(20),
            "--save" => a.save = true,
            "--json" => a.json = true,
            "-h" | "--help" => a.cmd = "help".into(),
            _ if a.target.is_none() => a.target = Some(x),
            other => bail!("unexpected argument: {other}"),
        }
    }
    Ok(a)
}

fn run() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("service") {
        return service::run(&raw[1..]);
    }
    let a = parse()?;
    match a.cmd.as_str() {
        "scan" | "top" | "export" => {
            let target = a.target.clone().unwrap_or_else(source::default_target);
            let idx = source::scan(&target, a.image.as_deref())?;
            if a.save {
                let p = gigamap_core::snapshot::store(&gigamap_core::snapshot::default_base(), &idx, 10)?;
                eprintln!("snapshot: {}", p.display());
            }
            match a.cmd.as_str() {
                "export" => report::export_tsv(&idx),
                "top" => print!("{}", report::top(&idx, 0, a.n)),
                _ if a.json => println!("{}", serde_json::to_string(&report::summary_json(&idx))?),
                _ => {
                    print!("{}", report::overview(&idx));
                    print!("{}", report::breakdown(&idx, 0, a.n));
                }
            }
            eprintln!(
                "{} items in {} ms via {:?}, {}",
                idx.len(),
                idx.scan_ms,
                idx.method.unwrap(),
                fmt::size(idx.total_size[0])
            );
            Ok(())
        }
        "mcp" => mcp::serve(),
        "selfcheck" => selfcheck(a.target.as_deref()),
        "" | "help" => {
            println!("{HELP}");
            Ok(())
        }
        other => bail!("unknown command `{other}`, see `gigamap help`"),
    }
}
