#!/bin/sh
# Usage: scripts/linux-check/run.sh [SOURCE_DIR] [STEP...]
#   SOURCE_DIR  a checkout of the repo (default: the one this script is in)
#   STEP        build test clippy snap release bloat  (default: build test clippy snap)
# The source is mounted read-only and copied (no .git, target) into the container. Cargo's registry and
# the target dir live in docker volumes (kot-linux-cargo, kot-linux-target), never on the host.
# Logs: /tmp/linux-check/<label>/<step>.log, label = the folder's name.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
src=$here/../..
if [ $# -gt 0 ] && [ -d "$1" ]; then src=$1; shift; fi
src=$(cd "$src" && pwd)
steps=${*:-build test clippy snap}
label=$(basename "$src")
out=/tmp/linux-check/$label
mkdir -p "$out"
docker build -q -t kot-linux-check "$here" >/dev/null
docker run --rm -v "$src":/src:ro -v "$out":/out \
  -v kot-linux-cargo:/cargo -v kot-linux-target:/target \
  -e STEPS="$steps" -e WGPU_BACKEND="${WGPU_BACKEND:-gl}" -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" kot-linux-check sh -c '
set -u
fail=0
mkdir -p /work "$HOME" && rsync -a --delete --exclude .git --exclude target /src/ /work/
cd /work
# Scratch everything the app could touch, though tests and snaps never open the socket.
export KOTORA_TEST_INSTANCE=1 KOTORA_OFFLINE=1 KOTORA_CTL_SOCKET=/tmp/kot-check/kotora.sock \
  XDG_CONFIG_HOME=/tmp/kot-check/config XDG_DATA_HOME=/tmp/kot-check/data \
  XDG_CACHE_HOME=/tmp/kot-check/cache XDG_STATE_HOME=/tmp/kot-check/state XDG_RUNTIME_DIR=/tmp/kot-check
mkdir -p /tmp/kot-check && chmod 700 /tmp/kot-check
for s in $STEPS; do
  case $s in
    build)   cmd="cargo build --workspace" ;;
    test)    cmd="sh -c \"cargo test --workspace && KOTORA_PRIVATE_BUS=1 dbus-run-session -- cargo test -p kotora-detect --test mpris_bus -- --nocapture\"" ;;
    clippy)  cmd="cargo clippy --workspace --all-targets" ;;
    snap)    cmd="sh -c \"for sc in list list-covers loading list-dropped settings-page list-editor queue-page now-playing-page now-playing-offer; do cargo run -q -p kotora -- --snap /out/\\\$sc.png --scene \\\$sc --size 1280x800 --scale 1 || exit 1; done\"" ;;
    release) cmd="sh -c \"cargo build --release -p kotora -p kotora-ctl && strip -o /tmp/kotora.stripped /target/release/kotora && strip -o /tmp/kot.stripped /target/release/kot && ls -l /target/release/kotora /tmp/kotora.stripped /tmp/kot.stripped\"" ;;
    bloat)   cmd="sh -c \"cargo install -q --locked cargo-bloat && cargo bloat --release -p kotora --crates -n 25\"" ;;
    *) echo "unknown step $s"; continue ;;
  esac
  echo "== $s: $cmd"
  eval "$cmd" >/out/$s.log 2>&1; rc=$?; echo "== $s exit $rc"
  [ $rc -eq 0 ] || fail=1
  if [ $s = clippy ]; then echo "clippy warnings: $(grep -c "^warning" /out/clippy.log)"; fi
  if [ $s = test ]; then grep -E "^test result|FAILED|failed" /out/test.log | sort | uniq -c | head -20; fi
  if [ $s = release ]; then tail -3 /out/release.log; fi
  if [ $s = bloat ]; then tail -30 /out/bloat.log; fi
done
exit ${fail:-0}'
