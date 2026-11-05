# Gigamap

See what fills your disk in seconds. Gigamap reads the NTFS master file table
directly instead of walking folders one by one, draws the whole drive as a
treemap and tells you what grew, what is safe to clean and what is duplicated.

It also ships an MCP server, so coding agents (Claude Code, Claude Desktop,
Cursor, anything that speaks MCP) can ask "what is eating my disk" and get a
short answer instead of crawling your folders themselves.

## Features

- **Fast.** NTFS drives are read straight from the MFT: hundreds of thousands of files per second.
  Other drives (exFAT, network shares) fall back to a parallel folder walk.
- **Treemap** with cushion shading, zoom into any folder, breadcrumbs, search.
- **Largest files and heaviest folders.** Heaviest folders never overlap, so their sizes add up.
- **Growth.** Every scan is saved; the next scan shows which folders grew and which big files appeared.
- **Cleanup candidates.** `node_modules`, Unity `Library`/`Temp`, cargo `target`, .NET `bin`/`obj`,
  Gradle and npm caches, browser caches, Windows Update leftovers, each marked safe / review / Windows.
- **Duplicates.** Same-size files are compared by content (BLAKE3), biggest waste first.
- **Recycle Bin only.** Nothing is ever deleted permanently.
- **No UAC prompts.** A small Gigamap service reads the file table; the app itself runs as you.

## Install

Grab `Gigamap_x.y.z_x64-setup.exe` or `Gigamap-portable.zip` from
[Releases](https://github.com/MRRengi/rng-gigamap/releases).

The installer also sets up the Gigamap service. With the portable build, the app
uses fast mode when started as administrator and the folder walk otherwise; you
can install the service yourself from an elevated terminal:

```
gigamap service install
```

## For coding agents (MCP)

Point your agent at the `gigamap` CLI that comes with the app:

```json
{
  "mcpServers": {
    "gigamap": {
      "command": "C:\\Program Files\\Gigamap\\gigamap.exe",
      "args": ["mcp"]
    }
  }
}
```

Claude Code: `claude mcp add gigamap -- "C:\Program Files\Gigamap\gigamap.exe" mcp`

Tools: `disk_overview`, `folder_breakdown`, `top_items`, `file_types`, `growth`,
`cleanup_candidates`, `find_duplicates`, `search`, `rescan`, `request_delete`, `delete_status`.

Agents can't delete anything on their own. `request_delete` shows a card in the
Gigamap window; files move to the Recycle Bin only if you press the button.

## CLI

```
gigamap scan C:            overview and top-level breakdown
gigamap top C: -n 20       largest files and heaviest folders
gigamap export D:\work     folder sizes as TSV
gigamap selfcheck C:\data  compare the MFT scan with a folder walk
```

## How it works

The MFT reader parses FILE records itself: update sequence fixups, `$STANDARD_INFORMATION`,
`$FILE_NAME` (Win32 names preferred, hard links counted once), `$DATA` including alternate streams
and sparse/compressed sizes, extension records, and `$Bitmap` for free space. It is tested against
real NTFS images built with `ntfs-3g` (`bench/ntfs.py`) and against folder walks on Windows in CI.

The service listens on a local named pipe (remote clients are rejected) and only answers
"scan drive X". Any signed-in local user can request a scan, which shows the file names on that
drive, the same as running a disk analyzer as administrator.

## License

MIT
