#!/usr/bin/env python3
"""Embed the WebAssembly module into web/index.html (one self-contained file).

Usage (from the repository root):
    cargo build -p terra-web --target wasm32-unknown-unknown --profile wasm --locked
    python3 web/build.py [path/to/terra_web.wasm]
    python3 web/build.py --check      # fails unless web/index.html equals a fresh build
"""
import base64
import hashlib
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parent.parent
default = root / "target" / "wasm32-unknown-unknown" / "wasm" / "terra_web.wasm"
args = [a for a in sys.argv[1:] if a != "--check"]
check = "--check" in sys.argv[1:]
wasm_path = pathlib.Path(args[0]) if args else default
if not wasm_path.exists():
    sys.exit(f"missing {wasm_path}; build the wasm module first (see the header of this file)")

wasm = wasm_path.read_bytes()
template = (root / "web" / "index.template.html").read_text(encoding="utf-8")
if "__WASM_BASE64__" not in template:
    sys.exit("placeholder __WASM_BASE64__ not found in web/index.template.html")
page = template.replace("__WASM_BASE64__", base64.b64encode(wasm).decode("ascii"))
target = root / "web" / "index.html"
if check:
    current = target.read_text(encoding="utf-8") if target.exists() else ""
    if current != page:
        sys.exit("web/index.html differs from a fresh build; run python3 web/build.py and commit the result")
    print(f"web/index.html matches a fresh build (wasm sha256 {hashlib.sha256(wasm).hexdigest()})")
    sys.exit(0)
target.write_text(page, encoding="utf-8")
print(f"web/index.html written: {len(page)} bytes, wasm {len(wasm)} bytes, sha256 {hashlib.sha256(wasm).hexdigest()}")
