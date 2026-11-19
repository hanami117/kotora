#!/bin/sh
# Screenshots of every screen, light and dark, from the booted Simulator (after run-sim.sh).
# Usage: ios/scripts/shots.sh <out-dir>
set -eu
out=${1:?usage: shots.sh <out-dir>}
mkdir -p "$out"
app=io.github.hanami117.kotora.spike
xcrun simctl status_bar booted override --time 18:30 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
shot() { # name, launch args…
  name=$1; shift
  xcrun simctl terminate booted "$app" 2>/dev/null || true
  xcrun simctl launch booted "$app" -resetSample "$@" >/dev/null
  sleep 2.5
  xcrun simctl io booted screenshot "$out/$name-$theme.png" >/dev/null 2>&1
  echo "$out/$name-$theme.png"
}
for theme in light dark; do
  xcrun simctl ui booted appearance "$theme"
  shot list -screen list
  shot list-undo -screen list -plusOne 7
  shot details -screen details -id 7
  shot airing -screen airing
  shot settings -screen settings
  shot search -screen search
done
xcrun simctl status_bar booted clear
