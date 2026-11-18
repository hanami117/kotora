#!/usr/bin/env bash
# Prepares a release of Kotora. It builds and writes files only; it never publishes anything
# (no `gh`, no `git push`). At the end it prints what to run and what to upload.
#
#   packaging/release.sh             checks, DMG (macOS), source tarball, checksums, notes, latest.json
#   packaging/release.sh --no-dmg    the same without the DMG (e.g. on Linux)
#   packaging/release.sh --no-dmg --linux-bin
#                                    also the prebuilt Linux x86_64 tarball for the AUR's kotora-bin
#                                    (build it on Linux x86_64; it is a release build of the tag).
#                                    The release notes list kotora-bin and this tarball either way, so
#                                    on a Mac the Linux step must follow before the release is final.
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
#   kotora-<release>-linux-x86_64.tar.gz (+ .sha256)
#                                          with --linux-bin: stripped binaries, desktop entry and icons,
#                                          top folder kotora-<version>-linux-x86_64/ (bin/, share/)
#   SHA256SUMS                             all of them, in `shasum -c` form
#   notes.md                               release notes (CHANGELOG section + install notes)
#   latest.json                            a first draft (no Windows lines); packaging/publish.sh writes
#                                          the real one after the release is public and complete
set -euo pipefail

cd "$(dirname "$0")/.."
say() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf 'release.sh: %s\n' "$*" >&2; exit 1; }

REPO=hanami117/kotora
# A stable file name (the same bytes as the versioned one) for the site's buttons (releases/latest/download/).
# latest.json never uses it: it names its own tag's versioned files.
DMG_LATEST=Kotora-macOS.dmg
dmg=1
linux_bin=0
while (($#)); do
    case "$1" in
        --no-dmg) dmg=0 ;;
        --linux-bin) linux_bin=1 ;;
        -h|--help) sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'; exit ;;
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

# --- Linux x86_64 binary tarball (for the AUR's kotora-bin) -------------------------------------------
bin=kotora-$release-linux-x86_64.tar.gz
if ((linux_bin)); then
    [[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || die "--linux-bin needs Linux on x86_64"
    command -v strip >/dev/null || die "--linux-bin needs strip (binutils)"
    say "Building the Linux x86_64 release binaries from $tag"
    # Its own target folder (the flags differ from a plain build, so nothing is shared). The home folder
    # is remapped so the binaries carry no path of the builder's. KOTORA_CHANNEL=aur-bin makes the
    # update check say "update with your AUR helper" (the install is under /usr).
    tgt=${CARGO_TARGET_DIR:-target}/linux-bin
    RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/build --remap-path-prefix=$PWD=/src" \
        KOTORA_CHANNEL=aur-bin CARGO_TARGET_DIR=$tgt \
        cargo build --release --locked -p kotora -p kotora-ctl
    id=io.github.hanami117.kotora
    top=kotora-$version-linux-x86_64
    stage=$out/stage/$top
    install -Dm755 -s "$tgt/release/kotora" "$stage/bin/kotora"
    install -Dm755 -s "$tgt/release/kot" "$stage/bin/kot"
    install -Dm644 "packaging/linux/$id.desktop" "$stage/share/applications/$id.desktop"
    install -Dm644 "assets/icons/$id.svg" "$stage/share/icons/hicolor/scalable/apps/$id.svg"
    for icon in assets/icons/hicolor/*/apps/$id.png; do
        size=$(basename "$(dirname "$(dirname "$icon")")")
        install -Dm644 "$icon" "$stage/share/icons/hicolor/$size/apps/$id.png"
    done
    install -m644 LICENSE THIRD_PARTY_NOTICES.md README.md "$stage/"
    # Same bytes on every run: sorted names, no owner, the tag's date, no name or time in gzip.
    tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$(git log -1 --format=%ct "$tag")" \
        -C "$out/stage" -cf - "$top" | gzip -9n >"$out/$bin"
    rm -rf "$out/stage"
fi

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
- **Arch Linux**: \`yay -S kotora\` (AUR, builds from this release's source tarball) or \`yay -S kotora-bin\` (AUR,
  the prebuilt binary from \`$bin\`, nothing to build). Install one of the two; they conflict.
- **macOS** (Apple silicon, macOS 13 or later): download \`Kotora-$release.dmg\` and drag Kotora to Applications.
  Kotora is not notarized, so macOS refuses it at the first start: open **System Settings → Privacy & Security**
  and click **Open Anyway** (or run \`xattr -dr com.apple.quarantine /Applications/Kotora.app\`).
- **Windows** (preview; Windows 10 or 11, 64-bit): download \`Kotora-$release-windows-x64-setup.exe\` and run it.
  It installs for your account only (no administrator rights) and puts \`kot\` on your PATH; a zip without an
  installer is here too. Kotora is not signed yet, so Windows SmartScreen says "Windows protected your PC":
  click **More info**, then **Run anyway**.
- **Other Linux**: download \`$src\`, unpack it and run \`./install.sh\` (needs Rust, pkgconf and a Vulkan
  driver; see the README). On x86_64 you can instead unpack the prebuilt \`$bin\` and copy its
  \`bin/\` and \`share/\` folders to \`/usr/local\` (or \`~/.local\`).

Sign in with MyAnimeList or AniList in one click: Kotora has its own client IDs built in, so there is nothing to register.
Checksums are in \`SHA256SUMS\`.
NOTES
} >"$out/notes.md"

# --- latest.json --------------------------------------------------------------------------------
# The format of core/src/update.rs (`Latest`); the site's site/latest.json and the update Worker serve it.
# Only versioned links (releases/download/<tag>/…), never releases/latest/…: a copy that reads this file
# must get exactly this version's files, even while the next release is being uploaded.
# The Windows installer is built by the `windows` workflow after the tag is pushed, so this first draft
# has no Windows lines. packaging/publish.sh writes the final latest.json (with them) once the release
# is public and complete; publish that one, not this.
dl=https://github.com/$REPO/releases/download/$tag
windows_lines=
if [[ -n ${WINDOWS_SHA256:-} ]]; then
  [[ $WINDOWS_SHA256 =~ ^[0-9a-f]{64}$ ]] || die "WINDOWS_SHA256 is not 64 lowercase hex digits"
  windows_lines=$(printf ',\n    "windows_installer": "%s",\n    "windows_installer_sha256": "%s"' "$dl/Kotora-$release-windows-x64-setup.exe" "$WINDOWS_SHA256")
fi
cat >"$out/latest.json" <<JSON
{
  "version": "$version",
  "build": $build,
  "date": "$(git log -1 --format=%cs "$tag")",
  "minimum_supported": "$min_supported",
  "notes_url": "https://github.com/$REPO/releases/tag/$tag",
  "downloads": {
    "macos_dmg": "$dl/Kotora-$release.dmg",
    "linux_tarball": "$dl/$src",
    "aur": "https://aur.archlinux.org/packages/kotora"$windows_lines
  }
}
JSON
python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$out/latest.json" 2>/dev/null || warn "latest.json didn't parse (no python3 to check?)"

# --- What to do next (nothing below has been run) -----------------------------------------------
shopt -s nullglob
files=$(printf '%s ' "$out"/*.dmg "$out"/*.tar.gz "$out"/*.sha256 "$out/SHA256SUMS")
cat <<DONE

$(say "Ready in $out. Nothing was published.")

1. Push the tag, then create the release AS A DRAFT (nobody sees it; the windows and linux-bin
   workflows add their 8 files to it):

   git push origin $tag
   gh release create $tag --repo $REPO --draft --title "$title" --notes-file $out/notes.md --verify-tag \\
       $files

2. When both workflows are green, publish: it checks all 15 files and their checksums, completes
   SHA256SUMS, publishes (not Latest), checks every link from outside, marks it Latest, and writes the
   final latest.json (versioned links, Windows sha256) into $out:

   packaging/publish.sh $tag --wait

   Only then: the site (site/latest.json from that file, and any version flip on the pages), and
   packaging/check-links.sh https://kotora.moe/latest.json after the deploy.

3. The AUR package:
   packaging/aur/PKGBUILD   pkgver=$version  _build=$build  sha256sums=('$src_sha')
                            pkgrel: 1 for a new version, else one more than the AUR's
                            then: makepkg --printsrcinfo > .SRCINFO, push to aur.archlinux.org/kotora.git
$(
if ((linux_bin)); then
cat <<BINPKG

4. The AUR package kotora-bin:
   packaging/aur-bin/PKGBUILD   pkgver=$version  _build=$build  sha256sums=('$(sha256 "$out/$bin")')
                                (the same pkgrel rule; push to aur.archlinux.org/kotora-bin.git)
BINPKG
else
cat <<NOBIN

4. The AUR package kotora-bin (the notes above already list it and $bin):
   not built here. On Linux x86_64, from the same tag, run: packaging/release.sh --no-dmg --linux-bin
   then upload $bin and its .sha256 to the release, append its line to the release's SHA256SUMS,
   and update packaging/aur-bin/PKGBUILD (pkgver, sha256sums). Until then \`yay -S kotora-bin\` and the
   binary tarball in the notes don't exist yet.
NOBIN
fi
)
DONE
