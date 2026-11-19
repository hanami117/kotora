#!/bin/sh
# Writes ios/build/demo.db: the made-up sample list, through kotora-core's own store
# (mobile/examples/demo_db.rs). The app copies it into its data folder on first launch.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$here/build"
cd "$here/../mobile"
cargo run -q --example demo_db -- "$here/build/demo.db" "$(date +%s)"
