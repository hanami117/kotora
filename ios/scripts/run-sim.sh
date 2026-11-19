#!/bin/sh
# Builds the app for the Simulator, boots an iPhone, installs and launches it.
# Usage: ios/scripts/run-sim.sh ["iPhone 17" [launch args, e.g. -screen airing]]  (run build-rust.sh and demo-db.sh first)
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
device=${1:-iPhone 17}; [ $# -gt 0 ] && shift
cd "$here"
xcodegen generate --quiet
xcodebuild -project Kotora.xcodeproj -scheme Kotora -configuration Debug \
  -destination "platform=iOS Simulator,name=$device" -derivedDataPath DerivedData \
  CODE_SIGNING_ALLOWED=NO build -quiet
app="$here/DerivedData/Build/Products/Debug-iphonesimulator/Kotora.app"
xcrun simctl boot "$device" 2>/dev/null || true
# Xcode 27 shows simulators in DeviceHub; older Xcodes have Simulator.app.
open -a Simulator 2>/dev/null || open -b com.apple.dt.Devices
xcrun simctl bootstatus "$device" -b >/dev/null
xcrun simctl install "$device" "$app"
xcrun simctl launch "$device" io.github.hanami117.kotora.spike "$@" >/dev/null
echo "launched on $device"
