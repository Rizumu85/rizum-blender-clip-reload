"""
inspect_clip.py — print layer info from a .clip file (local-only inspector).

Use this when a .clip is too big to upload but we need to see the
Layer table (especially LayerComposite integers and LayerName).

Usage (from the project folder):
    python inspect_clip.py "path\to\file.clip"

Prints one row per layer:
    MainId | LayerName | LayerType | LayerComposite | LayerOpacity | LayerMasking

Copy that output back into the chat — that's all I need to fill in
_BLEND_MAPPING. No file upload required.
"""

import os
import struct
import sqlite3
import sys
import tempfile


def main():
    if len(sys.argv) < 2:
        print("usage: python inspect_clip.py <path-to-clip>")
        sys.exit(1)
    clip_path = sys.argv[1]
    if not os.path.isfile(clip_path):
        print(f"not a file: {clip_path}")
        sys.exit(1)

    with open(clip_path, "rb") as f:
        data = f.read()

    if data[:8] != b"CSFCHUNK":
        print("not a .clip (missing CSFCHUNK magic)")
        sys.exit(1)

    # Walk chunks, find CHNKSQLi.
    sql_bytes = None
    pos = 24
    while pos < len(data):
        ctype = data[pos:pos + 8]
        csize = struct.unpack_from(">Q", data, pos + 8)[0]
        if ctype == b"CHNKSQLi":
            sql_bytes = data[pos + 16 : pos + 16 + csize]
            break
        pos += 16 + csize

    if not sql_bytes:
        print("no CHNKSQLi chunk found")
        sys.exit(1)

    # SQLite needs a file path.
    fd, db_path = tempfile.mkstemp(suffix=".sqlite")
    os.close(fd)
    try:
        with open(db_path, "wb") as f:
            f.write(sql_bytes)
        conn = sqlite3.connect(db_path)
        cur = conn.execute(
            "SELECT MainId, LayerName, LayerType, LayerComposite, "
            "LayerOpacity, LayerMasking FROM Layer ORDER BY MainId"
        )
        print(f"{'id':>4}  {'name':<30}  {'type':>4}  {'composite':>9}  "
              f"{'opacity':>7}  {'mask':>4}")
        print("-" * 70)
        for r in cur.fetchall():
            name = (r[1] or "")[:30]
            print(f"{r[0]:>4}  {name:<30}  {r[2]:>4}  {r[3]:>9}  "
                  f"{r[4]:>7}  {r[5]:>4}")
        conn.close()
    finally:
        try:
            os.unlink(db_path)
        except OSError:
            pass


if __name__ == "__main__":
    main()
