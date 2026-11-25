"""Write store paths as a static binary cache the browser page reads from its own origin.

For each path: <digest>.narinfo, <digest>.ls (the NAR listing, with each file's offset, which the
page uses to fetch a file without the whole NAR) and nar/<filehash>.nar.xz. The narinfos are not
signed: the page trusts its own origin as it trusts its own JavaScript, and checks every NAR
against its narinfo's NarHash.

Usage: make-cache.py <attrs.json> <graph key> <out dir> <store path>...
"""

import base64
import hashlib
import io
import json
import lzma
import os
import stat
import sys

ALPHABET = "0123456789abcdfghijklmnpqrsvwxyz"


def nix32(digest):
    length = (len(digest) * 8 + 4) // 5
    out = []
    for n in range(length - 1, -1, -1):
        b = n * 5
        i, j = b // 8, b % 8
        c = digest[i] >> j
        if i + 1 < len(digest):
            c |= digest[i + 1] << (8 - j)
        out.append(ALPHABET[c & 0x1F])
    return "".join(out)


class Nar:
    def __init__(self):
        self.buf = io.BytesIO()

    def raw(self, data):
        self.buf.write(len(data).to_bytes(8, "little"))
        self.buf.write(data)
        self.buf.write(b"\0" * (-len(data) % 8))

    def s(self, text):
        self.raw(text.encode())

    def node(self, path):
        st = os.lstat(path)
        self.s("(")
        self.s("type")
        if stat.S_ISLNK(st.st_mode):
            target = os.readlink(path)
            self.s("symlink")
            self.s("target")
            self.s(target)
            listing = {"type": "symlink", "target": target}
        elif stat.S_ISDIR(st.st_mode):
            self.s("directory")
            entries = {}
            for name in sorted(os.listdir(path), key=lambda n: n.encode()):
                self.s("entry")
                self.s("(")
                self.s("name")
                self.s(name)
                self.s("node")
                entries[name] = self.node(os.path.join(path, name))
                self.s(")")
            listing = {"type": "directory", "entries": entries}
        else:
            self.s("regular")
            executable = bool(st.st_mode & 0o100)
            if executable:
                self.s("executable")
                self.s("")
            self.s("contents")
            with open(path, "rb") as f:
                data = f.read()
            self.buf.write(len(data).to_bytes(8, "little"))
            offset = self.buf.tell()
            self.buf.write(data)
            self.buf.write(b"\0" * (-len(data) % 8))
            listing = {"type": "regular", "size": len(data), "narOffset": offset}
            if executable:
                listing["executable"] = True
        self.s(")")
        return listing


def main():
    attrs_file, key, out, *paths = sys.argv[1:]
    with open(attrs_file) as f:
        graph = {entry["path"]: entry for entry in json.load(f)[key]}
    os.makedirs(os.path.join(out, "nar"), exist_ok=True)
    for path in paths:
        nar = Nar()
        nar.s("nix-archive-1")
        listing = nar.node(path)
        data = nar.buf.getvalue()
        nar_hash = hashlib.sha256(data).digest()
        compressed = lzma.compress(data, preset=9 | lzma.PRESET_EXTREME)
        file_hash = nix32(hashlib.sha256(compressed).digest())
        with open(os.path.join(out, "nar", f"{file_hash}.nar.xz"), "wb") as f:
            f.write(compressed)

        info = graph[path]
        # The store's own hash of the path: a NAR written wrong would fail in the browser.
        expected = info.get("narHash", "")
        forms = (f"sha256:{nix32(nar_hash)}", "sha256-" + base64.b64encode(nar_hash).decode())
        if expected and expected not in forms:
            sys.exit(f"{path}: NAR hash {forms[1]} is not the store's {expected}")
        digest = os.path.basename(path)[:32]
        refs = sorted(os.path.basename(r) for r in info["references"])
        lines = [
            f"StorePath: {path}",
            f"URL: nar/{file_hash}.nar.xz",
            "Compression: xz",
            f"FileHash: sha256:{file_hash}",
            f"FileSize: {len(compressed)}",
            f"NarHash: sha256:{nix32(nar_hash)}",
            f"NarSize: {len(data)}",
            f"References: {' '.join(refs)}",
        ]
        with open(os.path.join(out, f"{digest}.narinfo"), "w") as f:
            f.write("\n".join(lines) + "\n")
        with open(os.path.join(out, f"{digest}.ls"), "w") as f:
            json.dump({"version": 1, "root": listing}, f, sort_keys=True)
        print(f"{path}: {len(data)} bytes, {len(compressed)} compressed", file=sys.stderr)


if __name__ == "__main__":
    main()
