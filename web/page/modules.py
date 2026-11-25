"""Copy the trynix modules an entry module reaches.

usage: modules.py <trynix js dir> <out dir> <entry>

Follows `from "./x.js"`, `import("./x.js")` and `new URL("./x.js", import.meta.url)` from the
entry (already in the out dir) and copies each module it reaches from the trynix directory, so the
site ships what the page runs and nothing else. A module the out dir already has is kept.
"""

import os
import re
import shutil
import sys

source, out, entry = sys.argv[1:]
LOCAL = re.compile(r"""(?:from\s+|import\s+|import\(\s*|new URL\(\s*)["'](\./[\w.-]+\.js)["']""")
# A worker's importScripts, whose names are relative to the worker's own directory.
SCRIPTS = re.compile(r"importScripts\(([^)]*)\)")

seen = set()
queue = [entry]
while queue:
    name = queue.pop()
    if name in seen:
        continue
    seen.add(name)
    target = os.path.join(out, name)
    if not os.path.exists(target):
        origin = os.path.join(source, name)
        if not os.path.exists(origin):
            sys.exit(f"{name} is imported but trynix has no such module")
        shutil.copy(origin, target)
        os.chmod(target, 0o644)
    text = open(target).read()
    for found in LOCAL.findall(text):
        queue.append(found[2:])
    for args in SCRIPTS.findall(text):
        for script in re.findall(r"""["']([\w.-]+\.js)["']""", args):
            queue.append(script)

print(f"modules: {len(seen)}: {' '.join(sorted(seen))}")
