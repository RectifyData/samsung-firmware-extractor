#!/usr/bin/env python3
"""
Builds synthetic Samsung-style firmware for the Rust round-trip tests.

Writes into rust/tests/fixtures/:
  SAMSUNG_TEST.zip, AP_TEST.tar.md5, loose.img.lz4   (firmware inputs)
  manifest.tsv                                        (firmware<TAB>image<TAB>sha256)

The expected hashes come from the raw payloads, produced independently of the Rust code,
so a passing test means the Rust decoder reproduced the exact bytes.
Covers: zip with nested tar.md5, GNU long names, USTAR prefixes, trailing "MD5",
LZ4 linked/independent blocks, block+content checksums, stored blocks, concatenated frames,
and Android sparse images (raw, fill, don't-care, CRC chunks).
"""
import hashlib, io, os, struct, sys, tarfile, zipfile
import lz4.frame as lz4f

OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "rust", "tests", "fixtures")


def sha(b):
    return hashlib.sha256(b).hexdigest()


def sparse_image():
    BLK = 4096
    r1 = os.urandom(4 * BLK)
    pat = b"\xde\xad\xbe\xef"
    r2 = os.urandom(2 * BLK)
    chunks = [
        (0xCAC1, 4, r1, r1),
        (0xCAC2, 8, pat, pat * (8 * BLK // 4)),
        (0xCAC3, 8, b"", b"\0" * (8 * BLK)),
        (0xCAC4, 0, struct.pack("<I", 0x12345678), b""),
        (0xCAC1, 2, r2, r2),
    ]
    body, expected = bytearray(), bytearray()
    for typ, nblk, payload, exp in chunks:
        body += struct.pack("<HHII", typ, 0, nblk, 12 + len(payload)) + payload
        expected += exp
    header = struct.pack("<IHHHHIIII", 0xED26FF3A, 1, 0, 28, 12, BLK, 22, len(chunks), 0)
    return header + bytes(body), bytes(expected)


def tar_bytes(members, fmt, trailer=b""):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=fmt) as t:
        for name, data in members:
            ti = tarfile.TarInfo(name)
            ti.size = len(data)
            ti.mode = 0o644
            t.addfile(ti, io.BytesIO(data))
    return buf.getvalue() + trailer


def main():
    os.makedirs(OUT, exist_ok=True)
    for f in os.listdir(OUT):
        os.remove(os.path.join(OUT, f))

    sparse_bytes, sparse_raw = sparse_image()
    payload = {
        "system.img": os.urandom(512 << 10) + b"\0" * (1 << 20) + b"abcd" * (128 << 10),
        "vendor.img": os.urandom(120 << 10),              # incompressible -> stored blocks
        "random.img": os.urandom(600 << 10),
        "sparse.img": sparse_raw,
        "multi.img": os.urandom(40 << 10) + b"multi-frame tail " * 2000,
        "loose.img": os.urandom(20000),
        "bl.img": os.urandom(4096),
        "x" * 110 + ".img": b"long-name payload " * 300,
        "y" * 90 + ".img": b"ustar-prefix payload " * 200,
    }
    expected = {name: sha(data) for name, data in payload.items()}

    lz = {
        "system": lz4f.compress(payload["system.img"]),
        "vendor": lz4f.compress(payload["vendor.img"], block_size=lz4f.BLOCKSIZE_MAX64KB,
                                block_linked=False, block_checksum=True, content_checksum=True),
        "random": lz4f.compress(payload["random.img"], block_size=lz4f.BLOCKSIZE_MAX256KB),
        "sparse": lz4f.compress(sparse_bytes),
        "multi": lz4f.compress(payload["multi.img"][:40 << 10])
                 + lz4f.compress(payload["multi.img"][40 << 10:]),
        "loose": lz4f.compress(payload["loose.img"]),
        "bl": lz4f.compress(payload["bl.img"]),
        "long": lz4f.compress(payload["x" * 110 + ".img"]),
        "ustar": lz4f.compress(payload["y" * 90 + ".img"]),
    }

    ap = tar_bytes([
        ("system.img.lz4", lz["system"]),
        ("vendor.img.lz4", lz["vendor"]),
        ("random.img.lz4", lz["random"]),
        ("sparse.img.lz4", lz["sparse"]),
        ("dir/" + "x" * 110 + ".img.lz4", lz["long"]),        # > 100 chars: GNU long name
        ("boot.img", os.urandom(1000)),                       # not lz4: must be ignored
    ], tarfile.GNU_FORMAT, trailer=b"MD5")
    bl = tar_bytes([
        ("firmware/" + "y" * 90 + ".img.lz4", lz["ustar"]),   # USTAR prefix split
        ("bl.img.lz4", lz["bl"]),
    ], tarfile.USTAR_FORMAT, trailer=b"MD5")
    cp = tar_bytes([("multi.img.lz4", lz["multi"])], tarfile.GNU_FORMAT, trailer=b"MD5")

    with open(os.path.join(OUT, "AP_TEST.tar.md5"), "wb") as f:
        f.write(ap)
    with open(os.path.join(OUT, "loose.img.lz4"), "wb") as f:
        f.write(lz["loose"])
    with zipfile.ZipFile(os.path.join(OUT, "SAMSUNG_TEST.zip"), "w") as z:
        z.writestr("AP_TEST.tar.md5", ap, compress_type=zipfile.ZIP_STORED)
        z.writestr("BL_TEST.tar.md5", bl, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("CP_TEST.tar.md5", cp, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("loose.img.lz4", lz["loose"], compress_type=zipfile.ZIP_STORED)
        z.writestr("meta-data", "FILE-VERSION=1\n")

    ap_images = ["system.img", "vendor.img", "random.img", "sparse.img", "x" * 110 + ".img"]
    bl_images = ["bl.img", "y" * 90 + ".img"]
    cp_images = ["multi.img"]
    rows = []
    for name in ap_images:
        rows.append(("AP_TEST.tar.md5", name, expected[name]))
    for name in bl_images + cp_images + ["loose.img"]:
        rows.append(("SAMSUNG_TEST.zip", name, expected[name]))
    for name in ap_images:
        rows.append(("SAMSUNG_TEST.zip", name, expected[name]))
    rows.append(("loose.img.lz4", "loose.img", expected["loose.img"]))
    with open(os.path.join(OUT, "manifest.tsv"), "w") as f:
        for fw, name, digest in rows:
            f.write(f"{fw}\t{name}\t{digest}\n")
    print(f"wrote {len(rows)} manifest rows to {os.path.abspath(OUT)}")


if __name__ == "__main__":
    main()
