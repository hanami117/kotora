#!/usr/bin/env bash
# Builds a minimal Kotora.app for this Mac, signed ad hoc (an app built on the Mac it runs on
# isn't quarantined, so it opens without Gatekeeper warnings). For live tests: it gives the Dock
# its name and icon, the menu bar its app name, and the login item a real app to start.
#
#   packaging/macos/build.sh          → target/macos/Kotora.app (release build)
#
# No disk image, no Homebrew, no notarisation, and it installs nothing: open it from the
# Finder where it is. Needs Rust and librsvg for the icon (`brew install librsvg`).
# Based on Uzumaki's packaging/macos/build.sh.
set -euo pipefail

cd "$(dirname "$0")/../.."
say() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
die() { printf 'build.sh: %s\n' "$*" >&2; exit 1; }

[[ $(uname -s) == Darwin ]] || die "run this on a Mac"
command -v rsvg-convert >/dev/null || die "rsvg-convert is missing (brew install librsvg)"

# The one version: [workspace.package] in Cargo.toml. BUILD is the build number (CFBundleVersion).
version=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[[ -n $version ]] || die "no version in Cargo.toml's [workspace.package]"
build=$(tr -d '[:space:]' <BUILD)
[[ $build =~ ^[1-9][0-9]*$ ]] || die "BUILD must hold a whole number above 0 (got '$build')"

target=${CARGO_TARGET_DIR:-target}
app=$target/macos/Kotora.app

say "Building Kotora $version (build $build, release)"
cargo build --release --locked -p kotora -p kotora-ctl
bin=$target/release

say "Assembling $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin/kotora" "$bin/kot" "$app/Contents/MacOS/"
cp LICENSE "$app/Contents/Resources/"

# The icon: the tile drawn at Apple's icon size (an 824 px body on a 1024 px canvas; the SVG's
# tile is 916 of its 1024 units, so the whole SVG is drawn 921 px wide), then every size.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
rsvg-convert -w 921 -h 921 --page-width 1024 --page-height 1024 --left 51 --top 51 \
    assets/icons/io.github.hanami117.kotora.svg -o "$work/icon-1024.png"
iconset=$work/Kotora.iconset
mkdir "$iconset"
for px in 16 32 128 256 512; do
    sips -z "$px" "$px" "$work/icon-1024.png" --out "$iconset/icon_${px}x${px}.png" >/dev/null
    sips -z "$((px * 2))" "$((px * 2))" "$work/icon-1024.png" --out "$iconset/icon_${px}x${px}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Kotora.icns"

cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleDisplayName</key><string>Kotora</string>
    <key>CFBundleExecutable</key><string>kotora</string>
    <key>CFBundleIconFile</key><string>Kotora</string>
    <key>CFBundleIdentifier</key><string>io.github.hanami117.kotora</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>Kotora</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$build</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.entertainment</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSUIElement</key><false/>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSHumanReadableCopyright</key><string>Free software under the GNU GPL, version 3 or later.</string>
</dict>
</plist>
PLIST
printf 'APPL????' >"$app/Contents/PkgInfo"

# Ad hoc, inside out, no --deep.
say "Signing (ad hoc)"
codesign --force --sign - "$app/Contents/MacOS/kot"
codesign --force --sign - "$app"
codesign --verify --strict "$app" || die "the bundle's signature doesn't verify"

say "Done: $(du -sh "$app" | cut -f1) $app"
say "Open it from the Finder (open $app). Quit any other running Kotora first (kot quit)."
