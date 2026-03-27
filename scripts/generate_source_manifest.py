"""Generate a JSON manifest of all tangled source files for the book's source viewer.

Parses markdown chapters to discover file= attributes in entangled code blocks,
reads the tangled files, strips marker comments, and writes a JSON manifest
to book/src/source-manifest.json.

Usage: python scripts/generate_source_manifest.py
"""

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BOOK_SRC = ROOT / "book" / "src"
OUTPUT = BOOK_SRC / "source-manifest.json"

# Regex to find entangled code block file attributes: ``` {.lang file=path}
# Only matches at the start of a line to avoid false positives in prose.
FILE_ATTR_RE = re.compile(r"^```\s*\{[^}]*file=([^\s}]+)", re.MULTILINE)

# Marker stripping regexes per comment style.
# Anchored to line start with re.MULTILINE to avoid corrupting mid-line content.
MARKER_PATTERNS = {
    ".rs": re.compile(r"^[ ]*// ~/~ [^\n]*\n", re.MULTILINE),
    ".toml": re.compile(r"^[ ]*# ~/~ [^\n]*\n", re.MULTILINE),
    ".lua": re.compile(r"^[ ]*-- ~/~ [^\n]*\n", re.MULTILINE),
}


def discover_files():
    """Parse all markdown files to find unique file= paths."""
    files = set()
    for md in sorted(BOOK_SRC.glob("**/*.md")):
        text = md.read_text()
        for match in FILE_ATTR_RE.finditer(text):
            files.add(match.group(1))
    if not files:
        print("Warning: no file= attributes found in any markdown files")
    return sorted(files)


def strip_markers(content, suffix):
    """Strip entangled marker lines from file content."""
    pattern = MARKER_PATTERNS.get(suffix)
    if pattern:
        return pattern.sub("", content)
    return content


def lang_from_suffix(suffix):
    """Map file extension to highlight language name."""
    return {
        ".rs": "rust",
        ".toml": "toml",
        ".lua": "lua",
    }.get(suffix, "text")


def build_tree(file_paths):
    """Build a nested directory tree structure from a list of file paths."""
    root_children = []

    def find_or_create_dir(children, name):
        for child in children:
            if child["type"] == "dir" and child["name"] == name:
                return child["children"]
        new_dir = {"name": name, "type": "dir", "children": []}
        children.append(new_dir)
        return new_dir["children"]

    for path_str in file_paths:
        parts = pathlib.PurePosixPath(path_str).parts
        current = root_children
        for part in parts[:-1]:
            current = find_or_create_dir(current, part)
        filename = parts[-1]
        suffix = pathlib.PurePosixPath(filename).suffix
        current.append(
            {
                "name": filename,
                "type": "file",
                "path": path_str,
                "lang": lang_from_suffix(suffix),
            }
        )

    def sort_tree(children):
        dirs = sorted([c for c in children if c["type"] == "dir"], key=lambda c: c["name"])
        files = sorted([c for c in children if c["type"] == "file"], key=lambda c: c["name"])
        for d in dirs:
            d["children"] = sort_tree(d["children"])
        return dirs + files

    return sort_tree(root_children)


def main():
    file_paths = discover_files()
    print(f"Discovered {len(file_paths)} tangled files from markdown")

    files = {}
    missing = []
    for rel_path in file_paths:
        abs_path = ROOT / rel_path
        if not abs_path.exists():
            missing.append(rel_path)
            continue
        try:
            content = abs_path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            print(f"Warning: skipping {rel_path} (not valid UTF-8)")
            continue
        content = strip_markers(content, abs_path.suffix)
        files[rel_path] = content

    if missing:
        print(f"Warning: {len(missing)} files not found (run tangle first?):")
        for m in missing:
            print(f"  - {m}")

    if not files and file_paths:
        print("Error: all discovered files are missing", file=sys.stderr)
        sys.exit(1)

    manifest = {
        "tree": build_tree(list(files.keys())),
        "files": files,
    }

    OUTPUT.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(f"Wrote {OUTPUT} ({len(files)} files)")


if __name__ == "__main__":
    main()
