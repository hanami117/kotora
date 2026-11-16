#!/usr/bin/env bash
# Prepares a release of Kotora. It builds and writes files only; it never publishes anything
# (no `gh`, no `git push`). At the end it prints what to run and what to upload.
#
#   packaging/release.sh             checks, DMG (macOS), source tarball, checksums, notes, latest.json
#   packaging/release.sh --no-dmg    the same without the DMG (e.g. on Linux)
#
# Before: the version in Cargo.toml's [workspace.package], the build number in BUILD, a CHANGELOG.md
# section "## [<version>]", everything committed, and an annotated (signed) tag on HEAD:
#   git tag -s v<version> -m "Kotora <version>"                           (build 1)
#   git tag -s v<version>-<build> -m "Kotora <version> (build <build>)"    (build 2 and up)
# The build number goes up when the packages are rebuilt without code changes: raise BUILD, commit,
# tag, run this again. A new version starts at build 1.
# packaging/MINIMUM_SUPPORTED (one version, e.g. 0.1.0) goes into latest.json as `minimum_supported`:
# older copies get a notice that can't be closed. Raise it ONLY when older versions are known to be
# broken (MyAnimeList changed its API, say); never for an ordinary release.
# Adapted from Uzumaki's packaging/release.sh.
#
# <release> below is <version> for build 1 and <version>-<build> after it.
# Out: target/release-<release>/
#   Kotora-<release>.dmg (+ .sha256)       macOS disk image, Apple silicon (packaging/macos/build.sh --dmg)
#   Kotora-macOS.dmg (+ .sha256)           the same file under a stable name (the site's button)
#   kotora-<release>.tar.gz (+ .sha256)    source tarball (`git archive` of the tag), top folder kotora-<version>/
#   SHA256SUMS                             all of them, in `shasum -c` form
#   notes.md                               release notes (CHANGELOG section + install notes)
#   latest.json                            for kotora.moe/latest.json and the update Worker
set -euo pipefail

cd "$(dirname "$0")/.."
say() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf 'release.sh: %s\n' "$*" >&2; exit 1; }

REPO=hanami117/kotora
# A stable file name (the same bytes as the versioned one) so links can say releases/latest/download/.
DMG_LATEST=Kotora-macOS.dmg
dmg=1
while (($#)); do
    case "$1" in
        --no-dmg) dmg=0 ;;
        -h|--help) sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'; exit ;;
        *) die "unknown option '$1' (try --help)" ;;
    esac
    shift
done

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

# --- Checks -------------------------------------------------------------------------------------
version=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "no semver version in Cargo.toml's [workspace.package] (got '$version')"
build=$(tr -d '[:space:]' <BUILD)
[[ $build =~ ^[1-9][0-9]*$ ]] || die "BUILD must hold a whole number above 0 (got '$build')"
if ((build == 1)); then release=$version; else release=$version-$build; fi
tag=v$release
title="Kotora $version"
((build == 1)) || title="Kotora $version (build $build)"
min_supported=$(tr -d '[:space:]' <packaging/MINIMUM_SUPPORTED 2>/dev/null) || die "packaging/MINIMUM_SUPPORTED is missing (one version, e.g. 0.1.0)"
[[ $min_supported =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] ||
    die "packaging/MINIMUM_SUPPORTED must hold one version like 0.1.0 (got '$min_supported')"
if [[ $(printf '%s\n%s\n' "${version%%-*}" "$min_supported" | sort -t. -k1,1n -k2,2n -k3,3n | tail -1) != "${version%%-*}" ]]; then
    die "packaging/MINIMUM_SUPPORTED ($min_supported) is above the version being released ($version)"
fi
grep -q "^## \[$version\] - [0-9]\{4\}-[0-9][0-9]-[0-9][0-9]" CHANGELOG.md || die "CHANGELOG.md has no dated '## [$version] - YYYY-MM-DD' section"

[[ -z $(git status --porcelain) ]] || die "the working tree isn't clean (commit or stash first):
$(git status --short)"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null ||
    die "no tag $tag. Create it on the release commit: git tag -s $tag -m \"$title\""
[[ $(git cat-file -t "$tag") == tag ]] || die "$tag is a lightweight tag; make it annotated: git tag -fs $tag -m \"$title\""
[[ $(git rev-parse "$tag^{commit}") == $(git rev-parse HEAD) ]] ||
    die "$tag isn't HEAD (it is $(git rev-parse --short "$tag^{commit}")); check out the tag or move it"
git verify-tag "$tag" >/dev/null 2>&1 || warn "$tag has no signature that verifies here"

out=${CARGO_TARGET_DIR:-target}/release-$release
rm -rf "$out"
mkdir -p "$out"

# --- macOS disk image ---------------------------------------------------------------------------
if ((dmg)); then
    [[ $(uname -s) == Darwin ]] || die "the DMG needs a Mac (use --no-dmg elsewhere)"
    [[ $(uname -m) == arm64 ]] || warn "this Mac isn't Apple silicon: the DMG will be for $(uname -m)"
    say "Building the macOS disk image"
    packaging/macos/build.sh --dmg
    cp "${CARGO_TARGET_DIR:-target}/macos/Kotora-$release.dmg" "$out/"
    cp "$out/Kotora-$release.dmg" "$out/$DMG_LATEST"
    [[ -z $(git status --porcelain) ]] || die "build.sh changed tracked files: $(git status --short)"
fi

# --- Source tarball -----------------------------------------------------------------------------
src=kotora-$release.tar.gz
say "Making the source tarball from $tag"
git archive --format=tar.gz --prefix="kotora-$version/" -o "$out/$src" "$tag"

# --- Checksums ----------------------------------------------------------------------------------
say "Writing checksums"
: >"$out/SHA256SUMS"
for f in "$out"/*.dmg "$out"/*.tar.gz; do
    [[ -e $f ]] || continue
    name=$(basename "$f")
    sum=$(sha256 "$f")
    printf '%s  %s\n' "$sum" "$name" >"$f.sha256"
    printf '%s  %s\n' "$sum" "$name" >>"$out/SHA256SUMS"
done
src_sha=$(sha256 "$out/$src")

# --- Release notes ------------------------------------------------------------------------------
say "Writing release notes from CHANGELOG.md"
{
    awk -v v="$version" '
        $0 ~ "^## \\[" v "\\]" { on = 1; next }
        on && (/^## \[/ || /^\[[^]]+\]: /) { exit }
        on { print }
    ' CHANGELOG.md | sed -e '/./,$!d'
    if ((build > 1)); then
        printf '\nThis is build %s of %s: the same code, packaged again.\n' "$build" "$version"
    fi
    cat <<NOTES

### Install
- **Arch Linux**: \`yay -S kotora\` (AUR, builds from this release's source tarball).
- **macOS** (Apple silicon, macOS 13 or later): download \`Kotora-$release.dmg\` and drag Kotora to Applications.
  Kotora is not notarized, so macOS refuses it at the first start: open **System Settings → Privacy & Security**
  and click **Open Anyway** (or run \`xattr -dr com.apple.quarantine /Applications/Kotora.app\`).
- **Other Linux**: download \`$src\`, unpack it and run \`./install.sh\` (needs Rust, pkgconf and a Vulkan
  driver; see the README).

You need your own MyAnimeList Client ID (free, a minute): the setup wizard walks you through it.
Checksums are in \`SHA256SUMS\`.
NOTES
} >"$out/notes.md"

# --- latest.json --------------------------------------------------------------------------------
# The format of core/src/update.rs (`Latest`); the site's site/latest.json and the update Worker serve it.
dl=https://github.com/$REPO/releases/download/$tag
latest=https://github.com/$REPO/releases/latest/download
cat >"$out/latest.json" <<JSON
{
  "version": "$version",
  "build": $build,
  "date": "$(git log -1 --format=%cs "$tag")",
  "minimum_supported": "$min_supported",
  "notes_url": "https://github.com/$REPO/releases/tag/$tag",
  "downloads": {
    "macos_dmg": "$latest/$DMG_LATEST",
    "linux_tarball": "$dl/$src",
    "aur": "https://aur.archlinux.org/packages/kotora"
  }
}
JSON
python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$out/latest.json" 2>/dev/null || warn "latest.json didn't parse (no python3 to check?)"

# --- What to do next (nothing below has been run) -----------------------------------------------
shopt -s nullglob
files=$(printf '%s ' "$out"/*.dmg "$out"/*.tar.gz "$out"/*.sha256 "$out/SHA256SUMS")
cat <<DONE

$(say "Ready in $out. Nothing was published.")

1. Push the tag, then create the release:

   git push origin $tag
   gh release create $tag --repo $REPO --title "$title" --notes-file $out/notes.md --verify-tag \\
       $files

2. Publish latest.json on the site (site/latest.json on kotora.moe's main), after the release is up.
   The site's download button should use the stable link: $latest/$DMG_LATEST

$(sed 's/^/   /' "$out/latest.json")

3. The AUR package:
   packaging/aur/PKGBUILD   pkgver=$version  _build=$build  sha256sums=('$src_sha')
                            pkgrel: 1 for a new version, else one more than the AUR's
                            then: makepkg --printsrcinfo > .SRCINFO, push to aur.archlinux.org/kotora.git
DONE
