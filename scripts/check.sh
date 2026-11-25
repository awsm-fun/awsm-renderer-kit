#!/bin/sh
# Everything the kit is held to: formatting, native tests (Rust and the JS
# twins), clippy with warnings as errors (wasm32, every feature; and native),
# and a wasm32 check of each crate with each feature combination.
#
# awsm-renderer's `meshopt` dependency compiles C++ to wasm and needs a clang
# that can target it. Set CC/CXX/AR_wasm32_unknown_unknown yourself, or have
# Emscripten (`em-config LLVM_ROOT`) or Homebrew's LLVM installed.
set -eu
cd "$(dirname "$0")/.."

if [ -z "${CC_wasm32_unknown_unknown:-}" ]; then
  llvm=$(em-config LLVM_ROOT 2>/dev/null || true)
  [ -n "$llvm" ] || llvm=/opt/homebrew/opt/llvm/bin
  export CC_wasm32_unknown_unknown="$llvm/clang"
  export CXX_wasm32_unknown_unknown="$llvm/clang++"
  export AR_wasm32_unknown_unknown="$llvm/llvm-ar"
fi
wasm="--target wasm32-unknown-unknown"

echo "== fmt"
cargo fmt --all --check
echo "== cargo test (native)"
cargo test --workspace --quiet
echo "== node --test (the JS twins)"
node --test js/test/*.test.mjs
echo "== clippy (wasm32, all features)"
cargo clippy --workspace --all-features $wasm --quiet -- -D warnings
echo "== clippy (native, all targets)"
cargo clippy --workspace --all-targets --quiet -- -D warnings
echo "== wasm32 check per feature combination"
for spec in \
  "settings:" "settings:renderer" "settings:scene" "settings:web" "settings:ui" \
  "settings:renderer,ui" "settings:scene,ui" \
  "stats:" "stats:renderer" "stats:web" "stats:ui" "stats:renderer,ui"; do
  crate="awsm-renderer-${spec%%:*}"
  features="${spec#*:}"
  echo "   $crate [${features:-none}]"
  if [ -n "$features" ]; then
    cargo check -p "$crate" $wasm --quiet --no-default-features --features "$features"
  else
    cargo check -p "$crate" $wasm --quiet --no-default-features
  fi
done
echo "== all good"
