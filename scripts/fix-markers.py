"""Strip leading whitespace from entangled markers in tangled source files.

cargo fmt indents `// ~/~ begin` and `// ~/~ end` markers to match the
surrounding code, but entangled's stitch expects them at their original
indentation (column 0 for top-level file blocks). This script restores
that by stripping leading whitespace from marker lines.

Usage: python scripts/fix-markers.py
"""

import pathlib
import re

MARKER_RE = re.compile(r"^(\s+)(// ~/~ .*)$", re.MULTILINE)

changed = 0
for path in pathlib.Path("src").rglob("*.rs"):
    text = path.read_text()
    fixed = MARKER_RE.sub(r"\2", text)
    if fixed != text:
        path.write_text(fixed)
        changed += 1
        print(f"  fixed markers in {path}")

if changed == 0:
    print("  all markers already at correct indentation")
