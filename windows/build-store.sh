#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
pack_tool="${MAKEMSIX:-$HOME/.local/lib/dockodile-msix/bin/makemsix}"
export LD_LIBRARY_PATH="$(dirname -- "$pack_tool")/../lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
[[ -x "$pack_tool" ]] || { echo 'Microsoft makemsix fehlt. MAKEMSIX auf das SDK-Werkzeug setzen.' >&2; exit 1; }
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["package"]["version"])')
outdir="${1:-../release/store/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p -- "$outdir"
outdir=$(realpath -- "$outdir")
package="$outdir/Dockodile-$version-x64.msix"
[[ ! -e "$package" ]] || { echo "Datei existiert bereits: $package" >&2; exit 1; }
cargo build --release --locked --target x86_64-pc-windows-gnu --features microsoft-store --target-dir target-store
binary=target-store/x86_64-pc-windows-gnu/release/dockodile.exe
python3 ../tests/check_store_binary.py "$binary"
staging=$(mktemp -d)
trap 'rm -rf -- "$staging"' EXIT
cp -R msix/. "$staging/"
# Der eingereichte Publisher und Paketname bleiben erhalten; nur die Version folgt Cargo.
python3 - "$staging/AppxManifest.xml" "$version" <<'PY'
import pathlib,re,sys
p=pathlib.Path(sys.argv[1])
p.write_text(re.sub(r'(<Identity\b[^>]*\bVersion=")[^"]+',lambda m:m[1]+sys.argv[2]+'.0',p.read_text()))
PY
cp "$binary" "$staging/Dockodile.exe"
cp ../LICENSE "$staging/LICENSE.txt"
"$pack_tool" pack -d "$staging" -p "$package"
sha256sum "$package"
