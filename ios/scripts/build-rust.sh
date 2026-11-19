#!/bin/sh
# Builds kotora-mobile for iOS as a static library, generates the Swift bindings with UniFFI and
# packages KotoraCore.xcframework (device + Simulator) under ios/build/.
# Usage: ios/scripts/build-rust.sh [release|debug] [--sim-only]
# kotora-mobile's crate types (cdylib, lib) are left as they are: `cargo rustc --crate-type
# staticlib` asks for the static library from the command line.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
mobile="$here/../mobile"
out="$here/build"
profile=${1:-release}
targets="aarch64-apple-ios-sim aarch64-apple-ios"
[ "${2:-}" = "--sim-only" ] && targets="aarch64-apple-ios-sim"
flag=""; [ "$profile" = release ] && flag="--release"
export IPHONEOS_DEPLOYMENT_TARGET=17.0

cd "$mobile"
for t in $targets; do
  cargo rustc -p kotora-mobile --lib $flag --target "$t" --crate-type staticlib
done

# Bindings from a host build: it keeps the UniFFI metadata (as the Android build does).
cargo build -p kotora-mobile --lib
host_lib="$mobile/target/debug/libkotora_mobile.dylib"
rm -rf "$out/bindings" "$out/KotoraCore.xcframework"
mkdir -p "$out/bindings"
cargo run -q -p uniffi-bindgen -- generate --library "$host_lib" --language swift --out-dir "$out/bindings"

# A module map named for the framework, and the headers for the XCFramework.
hdr="$out/headers"
rm -rf "$hdr" && mkdir -p "$hdr"
cp "$out/bindings/"*FFI.h "$hdr/"
cp "$out/bindings/"*FFI.modulemap "$hdr/module.modulemap"
args=""
for t in $targets; do
  args="$args -library $mobile/target/$t/$profile/libkotora_mobile.a -headers $hdr"
done
xcodebuild -create-xcframework $args -output "$out/KotoraCore.xcframework"
mkdir -p "$here/Kotora/Generated"
cp "$out/bindings/"*.swift "$here/Kotora/Generated/"
echo "ok: $out/KotoraCore.xcframework"
