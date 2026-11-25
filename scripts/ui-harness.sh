#!/bin/sh
# Builds examples/ui-harness (the Rust panels on a page with fake data) into
# <out-dir> and serves it: scripts/ui-harness.sh <out-dir> [port]
# Needs the wasm-bindgen CLI of the locked version.
set -eu
kit_dir=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: scripts/ui-harness.sh <out-dir> [port]}
port=${2:-8124}
cd "$kit_dir"
cargo build -p awsm-renderer-kit-ui-harness --target wasm32-unknown-unknown --release
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-name harness --out-dir "$out" \
  target/wasm32-unknown-unknown/release/awsm_renderer_kit_ui_harness.wasm
cp examples/ui-harness/web/index.html "$out/"
style="$kit_dir/../templates/simple-multithreaded-c/dom/runtime/style.css"
[ -f "$style" ] && cp "$style" "$out/style.css"
echo "serving $out at http://127.0.0.1:$port/"
exec python3 -m http.server "$port" --bind 127.0.0.1 --directory "$out"
