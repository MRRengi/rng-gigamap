//! Talking to the elevated Gigamap service over `\\.\pipe\gigamap`.
//!
//! Request: one JSON line, e.g. `{"cmd":"scan","drive":"C"}`.
//! Reply: `u64` length + snapshot bytes (see `snapshot::write_to`),
//! or length 0 followed by `u32` length + UTF-8 error text.

use crate::Index;
use anyhow::{bail, Result};
use std::io::{Read, Write};

pub const PIPE: &str = r"\\.\pipe\gigamap";

pub fn write_reply<W: Write>(mut w: W, result: Result<&Index>) -> Result<()> {
    match result {
        Ok(idx) => {
            let buf = crate::snapshot::write_to(idx, Vec::new())?;
            w.write_all(&(buf.len() as u64).to_le_bytes())?;
            w.write_all(&buf)?;
        }
        Err(e) => {
            let msg = format!("{e:#}");
            w.write_all(&0u64.to_le_bytes())?;
            w.write_all(&(msg.len() as u32).to_le_bytes())?;
            w.write_all(msg.as_bytes())?;
        }
    }
    w.flush()?;
    Ok(())
}

pub fn read_reply<R: Read>(mut r: R) -> Result<Index> {
    let mut l = [0u8; 8];
    r.read_exact(&mut l)?;
    let n = u64::from_le_bytes(l) as usize;
    if n == 0 {
        let mut e = [0u8; 4];
        r.read_exact(&mut e)?;
        let mut msg = vec![0u8; u32::from_le_bytes(e) as usize];
        r.read_exact(&mut msg)?;
        bail!("service: {}", String::from_utf8_lossy(&msg));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    crate::snapshot::read_from(&buf[..])
}

/// Ask the running service for an MFT scan of a drive. Fails fast if the service is absent.
pub fn scan_via_service(drive: char) -> Result<Index> {
    let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(PIPE)?;
    writeln!(pipe, "{}", serde_json::json!({"cmd": "scan", "drive": drive.to_string()}))?;
    pipe.flush()?;
    read_reply(std::io::BufReader::new(pipe))
}

#[cfg(test)]
mod tests {
    #[test]
    fn reply_roundtrip() {
        let mut b = crate::IndexBuilder::new("C:\\", '\\', crate::ScanMethod::Mft);
        b.push(0, "a.bin", 77, 4096, 0, 0);
        let i = b.finish(std::time::Instant::now());
        let mut buf = Vec::new();
        super::write_reply(&mut buf, Ok(&i)).unwrap();
        assert_eq!(super::read_reply(&buf[..]).unwrap().total_size[0], 77);
        let mut buf = Vec::new();
        super::write_reply(&mut buf, Err(anyhow::anyhow!("nope"))).unwrap();
        assert!(super::read_reply(&buf[..]).err().unwrap().to_string().contains("nope"));
    }
}
