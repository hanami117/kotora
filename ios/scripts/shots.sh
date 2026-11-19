#!/bin/sh
# Screenshots of every screen, light and dark, from the booted Simulator (after run-sim.sh).
# Sample data ("Look around", debug builds) for the signed-in pages; the sign-in page as it is.
# Usage: ios/scripts/shots.sh <out-dir>
set -eu
out=${1:?usage: shots.sh <out-dir>}
mkdir -p "$out"
app=io.github.hanami117.kotora.debug
xcrun simctl status_bar booted override --time 18:30 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
shot() { # name, launch args…
  name=$1; shift
  xcrun simctl terminate booted "$app" 2>/dev/null || true
  xcrun simctl launch booted "$app" "$@" >/dev/null
  sleep 2.5
  xcrun simctl io booted screenshot "$out/$name-$theme.png" >/dev/null 2>&1
  echo "$out/$name-$theme.png"
}
for theme in light dark; do
  xcrun simctl ui booted appearance "$theme"
  shot signin -sample NO
  shot list -sample YES -screen list
  shot list-undo -sample YES -screen list -plusOne 7
  shot details -sample YES -screen details -id 7
  shot details-rewatch -sample YES -screen details -id 19
  shot airing -sample YES -screen airing
  shot settings -sample YES -screen settings
done
xcrun simctl status_bar booted clear
