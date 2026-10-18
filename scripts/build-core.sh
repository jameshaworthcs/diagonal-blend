#!/bin/sh
# Build the Rust core to WebAssembly and generate its JavaScript bindings
# into web/src/pkg, where Vite picks them up.
set -eu
cd "$(dirname "$0")/.."

cargo build --manifest-path core/Cargo.toml --target wasm32-unknown-unknown --release
wasm-bindgen --target bundler --out-dir web/src/pkg --out-name core \
  core/target/wasm32-unknown-unknown/release/diagonal_blend_core.wasm

# Shrink the module when binaryen is installed (it is a dev dependency).
if [ -x node_modules/.bin/wasm-opt ]; then
  node_modules/.bin/wasm-opt -Os --strip-debug \
    --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals --enable-reference-types --enable-multivalue \
    web/src/pkg/core_bg.wasm -o web/src/pkg/core_bg.wasm
fi
ls -l web/src/pkg/core_bg.wasm | awk '{ printf "core_bg.wasm: %.0f KB\n", $5 / 1024 }'
