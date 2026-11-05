#!/usr/bin/env python3
"""NTFS ground-truth bench for the MFT reader (Linux, needs mkntfs + ntfs-3g).

Builds a real NTFS image, fills it through the ntfs-3g driver from a generated
tree whose sizes are known in advance, scans the image with `gigamap export
--image`, and checks every folder total and file count against the manifest.

usage: bench/ntfs.py [--files N] [--keep] [--bin target/release/gigamap]
"""
import argparse, os, random, subprocess, sys, tempfile, time

NAMES = ["Projects", "Видео", "Документи", "🎮 Games", "node_modules", "Library",
         "a" * 120, "with space", "Ünïcödé", "日本語", "x.y.z", "build"]


def gen(root, n_files, rng, tiny=False):
    """Create the tree under `root`; return {relpath_dir: [size, files]} manifest."""
    manifest = {"": [0, 0]}

    def add(rel_dir, size, count=1):
        parts = rel_dir.split("/") if rel_dir else []
        for i in range(len(parts) + 1):
            key = "/".join(parts[:i])
            m = manifest.setdefault(key, [0, 0])
            m[0] += size
            m[1] += count

    dirs = [""]
    for i in range(max(8, n_files // 40)):
        parent = rng.choice(dirs) if len(dirs) < 3 or rng.random() < 0.8 else ""
        if parent.count("/") >= 7:
            parent = ""
        name = f"{rng.choice(NAMES)}_{i}"
        rel = f"{parent}/{name}" if parent else name
        os.makedirs(os.path.join(root, rel), exist_ok=True)
        manifest.setdefault(rel, [0, 0])
        dirs.append(rel)
    dirs = dirs[1:]

    def write(rel_dir, name, size):
        p = os.path.join(root, rel_dir, name)
        with open(p, "wb") as f:
            if size:
                f.write(rng.randbytes(min(size, 4096)) * (size // 4096) + rng.randbytes(size % 4096))
        add(rel_dir, size)

    for i in range(n_files):
        r = rng.random()
        size = 0 if r < 0.05 else rng.randint(1, 600) if r < 0.5 else rng.randint(601, 200_000) if r < 0.97 else rng.randint(1 << 20, 6 << 20)
        if tiny:
            size = min(size, 600)
        write(rng.choice(dirs), f"file_{i}.{rng.choice(['bin','txt','png','mp4','dll','unitypackage'])}", size)

    # One huge folder: forces a big $INDEX_ALLOCATION.
    big = "BigFolder"
    os.makedirs(os.path.join(root, big))
    manifest.setdefault(big, [0, 0])
    for i in range(5000):
        write(big, f"entry_{i:05d}_{'x' * (i % 40)}.dat", (i * 37) % 3000)

    # Fragmented files: interleave appends to two files.
    frag = "Fragmented"
    os.makedirs(os.path.join(root, frag))
    manifest.setdefault(frag, [0, 0])
    fa, fb = [open(os.path.join(root, frag, n), "ab") for n in ("a.bin", "b.bin")]
    sa = sb = 0
    for _ in range(64):
        fa.write(rng.randbytes(65536)); fa.flush(); sa += 65536
        fb.write(rng.randbytes(32768)); fb.flush(); sb += 32768
    fa.close(); fb.close()
    add(frag, sa); add(frag, sb)

    # Many hard links in one folder: the base record overflows into extension records.
    hl = "HardLinks"
    os.makedirs(os.path.join(root, hl))
    manifest.setdefault(hl, [0, 0])
    target = os.path.join(root, hl, "original.bin")
    with open(target, "wb") as f:
        f.write(rng.randbytes(123_457))
    add(hl, 123_457)
    for i in range(60):
        os.link(target, os.path.join(root, hl, f"link_{i:02d}_" + "L" * 60 + ".bin"))

    # Alternate data streams count toward logical size.
    ads = "Streams"
    os.makedirs(os.path.join(root, ads))
    manifest.setdefault(ads, [0, 0])
    p = os.path.join(root, ads, "host.txt")
    with open(p, "wb") as f:
        f.write(b"h" * 1000)
    with open(p + ":Zone.Identifier", "wb") as f:
        f.write(b"z" * 300)
    with open(p + ":big", "wb") as f:
        f.write(rng.randbytes(70_000))
    add(ads, 1000 + 300 + 70_000)

    # Sparse file: logical size is the apparent size.
    sp = "Sparse"
    os.makedirs(os.path.join(root, sp))
    manifest.setdefault(sp, [0, 0])
    with open(os.path.join(root, sp, "disk.vhd"), "wb") as f:
        f.write(b"head")
        f.truncate(50 << 20)
    add(sp, 50 << 20)
    return manifest


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--files", type=int, default=3000)
    ap.add_argument("--bin", default=os.path.join(os.path.dirname(__file__), "..", "target", "release", "gigamap"))
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--tiny", action="store_true", help="files <= 600 B, for record-count throughput runs")
    a = ap.parse_args()
    rng = random.Random(a.seed)

    work = tempfile.mkdtemp(prefix="gm-ntfs-")
    img, mnt = os.path.join(work, "vol.img"), os.path.join(work, "mnt")
    os.makedirs(mnt)
    img_size = (1 << 30) + a.files * (8_000 if a.tiny else 250_000)
    with open(img, "wb") as f:
        f.truncate(img_size)
    subprocess.run(["mkntfs", "-F", "-Q", "-q", "-L", "BENCH", img], check=True, stderr=subprocess.DEVNULL)
    subprocess.run(["ntfs-3g", "-o", "streams_interface=windows", img, mnt], check=True)
    t0 = time.time()
    try:
        manifest = gen(mnt, a.files, rng, a.tiny)
    finally:
        subprocess.run(["umount", mnt], check=True)
    print(f"image ready: {a.files + 5000} files, {len(manifest)} folders, filled in {time.time() - t0:.1f}s")

    t0 = time.time()
    out = subprocess.run([a.bin, "export", "X:", "--image", img], check=True, capture_output=True, text=True)
    wall = time.time() - t0
    got = {}
    for line in out.stdout.splitlines():
        path, size, files = line.split("\t")
        rel = path[3:].replace("\\", "/") if path.startswith("X:\\") else path
        got[rel] = (int(size), int(files))

    bad = 0
    for rel, (size, files) in sorted(manifest.items()):
        if rel == "":
            continue  # root also holds NTFS metadata files
        g = got.get(rel)
        if g != (size, files):
            bad += 1
            if bad <= 15:
                print(f"MISMATCH {rel!r}: expected {size} B / {files} files, got {g}")
    top = sum(v[0] for k, v in manifest.items() if k and "/" not in k)
    got_top = sum(v[0] for k, v in got.items() if k and "/" not in k and not k.startswith("$"))
    if top != got_top:
        bad += 1
        print(f"MISMATCH top-level sum: expected {top}, got {got_top}")
    print(out.stderr.strip())
    print(f"scan wall time {wall * 1000:.0f} ms, {len(manifest) - 1} folders checked, {bad} mismatches")
    if not a.keep:
        os.remove(img)
    else:
        print("image kept at", img)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
