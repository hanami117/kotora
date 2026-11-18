#!/usr/bin/env bash
# Publishes a release of Kotora that is complete, and only then. Run it from the kotora checkout (the tag
# must be here) after the windows and linux-bin workflows have uploaded their files to the DRAFT release.
#
#   packaging/publish.sh v0.5.0 --dry-run   check everything, change nothing, say what would be done
#   packaging/publish.sh v0.5.0             the same checks, then publish
#   packaging/publish.sh v0.5.0 --wait      first wait (up to 60 min) for the CI files to be there
#
# The rule: no link may point to something that isn't there yet. So, in this order:
#   1. The release is a draft (nobody sees it) and has exactly the 15 expected files, each `.sha256`
#      is GitHub's own digest of its file, the stable names are the same bytes as the versioned ones,
#      and SHA256SUMS lists all seven files (on a draft this script completes it from the .sha256 files).
#   2. Publish it, NOT as Latest: releases/latest and the site's buttons (…/latest/download/<stable name>)
#      still give the previous release, which is complete.
#   3. HEAD every file's versioned URL from outside (no token): all must answer 200. Else stop here: the
#      release is public but not Latest, and nothing links to it yet.
#   4. Mark it Latest. Check releases/latest and every stable name now lead to this tag and answer 200.
#   5. Write target/release-<release>/latest.json with versioned links only (releases/download/<tag>/…)
#      and the Windows installer's sha256 from the release, and run packaging/check-links.sh on it.
# Then (by hand, see the end of the output): copy latest.json to kotora.moe's site/latest.json, push,
# and run check-links.sh on the live URLs.
#
# A prerelease (version with a '-') is published as a prerelease and never marked Latest.
# Running it again on a release that is already public only checks it (and writes latest.json).
# GH_TOKEN must be a token of the repo's owner (on the Mac: GH_TOKEN=$(gh auth token --user hanami117)).
# REPO=<owner/name> points it at another repo (a private scratch copy for tests).
set -euo pipefail

cd "$(dirname "$0")/.."
say() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
die() { printf 'publish.sh: %s\n' "$*" >&2; exit 1; }
command -v gh >/dev/null || die "needs gh"
command -v jq >/dev/null || die "needs jq"

REPO=${REPO:-hanami117/kotora}
tag='' dry=0 wait=0
while (($#)); do
    case "$1" in
        --dry-run) dry=1 ;;
        --wait) wait=1 ;;
        -h|--help) sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'; exit ;;
        v*) tag=$1 ;;
        *) die "unknown argument '$1' (try --help)" ;;
    esac
    shift
done
[[ $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?)(-([0-9]+))?$ ]] || die "usage: packaging/publish.sh v<release> [--dry-run] [--wait]"
release=${tag#v}
git rev-parse -q --verify "refs/tags/$tag" >/dev/null || die "no tag $tag here (git fetch --tags)"
version=$(git show "$tag:Cargo.toml" | sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' | head -1)
build=$(git show "$tag:BUILD" | tr -d '[:space:]')
if [[ $build == 1 ]]; then want=$version; else want=$version-$build; fi
[[ $want == "$release" ]] || die "$tag doesn't match its sources (Cargo.toml + BUILD say v$want)"
prerelease=0; [[ $version == *-* ]] && prerelease=1

dl=https://github.com/$REPO/releases/download/$tag
# The 15 files: 7 from release.sh (Mac), 6 from the windows workflow, 2 from linux-bin.
files=(
    "Kotora-$release.dmg" Kotora-macOS.dmg "kotora-$release.tar.gz"
    "Kotora-$release-windows-x64-setup.exe" Kotora-windows-x64-setup.exe "Kotora-$release-windows-x64.zip"
    "kotora-$release-linux-x86_64.tar.gz"
)
expected=(SHA256SUMS)
for f in "${files[@]}"; do expected+=("$f" "$f.sha256"); done
# Same bytes under two names: the versioned file and the stable name the site's buttons use.
stable_pairs=("Kotora-$release.dmg Kotora-macOS.dmg" "Kotora-$release-windows-x64-setup.exe Kotora-windows-x64-setup.exe")

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

view() { gh release view "$tag" -R "$REPO" --json isDraft,isPrerelease,assets,url >"$tmp/rel.json"; }
digest() { jq -r --arg n "$1" '.assets[] | select(.name == $n) | .digest // "" | sub("^sha256:"; "")' "$tmp/rel.json"; }
missing() {
    local f
    for f in "${expected[@]}"; do
        jq -e --arg n "$f" '.assets[] | select(.name == $n and .state == "uploaded")' "$tmp/rel.json" >/dev/null || echo "$f"
    done
}
http_code() { curl -sSIL -o /dev/null -w '%{http_code}' --max-time 30 "$1" 2>/dev/null || true; }
# Every URL must answer 200 from outside (HEAD, no token, no cookie). The ones that don't are tried
# again together, four rounds 15 s apart, for a file that is still reaching the CDN.
heads_ok() {
    local round url code pending=("$@") left
    for round in 1 2 3 4; do
        left=()
        for url in "${pending[@]}"; do
            code=$(http_code "$url")
            if [[ $code == 200 ]]; then printf '  200  %s\n' "$url"
            elif ((round == 4)); then printf '  %s  %s\n' "${code:-000}" "$url"; left+=("$url")
            else left+=("$url"); fi
        done
        ((${#left[@]} == 0)) && return 0
        pending=("${left[@]}")
        ((round < 4)) && { echo "  ${#left[@]} not 200 yet, again in 15 s"; sleep 15; }
    done
    return 1
}

# --- 1. The release and its files ----------------------------------------------------------------
say "Release $tag on $REPO"
view || die "no release $tag on $REPO (create the draft: gh release create $tag --draft …)"
if ((wait)); then
    for i in $(seq 1 120); do
        m=$(missing)
        [[ -z $m ]] && break
        echo "waiting for $(wc -l <<<"$m" | tr -d ' ') file(s) ($i/120): $(tr '\n' ' ' <<<"$m")"
        sleep 30
        view
    done
fi
m=$(missing)
[[ -z $m ]] || die "the release doesn't have every file yet. Missing:
$m
(check the windows and linux-bin runs: gh run list -R $REPO --limit 6)"
extra=$(jq -r '.assets[].name' "$tmp/rel.json" | grep -vxF -f <(printf '%s\n' "${expected[@]}") || true)
[[ -z $extra ]] || die "the release has files that aren't expected: $extra"
draft=$(jq -r .isDraft "$tmp/rel.json")
echo "15 files there; draft: $draft, prerelease: $(jq -r .isPrerelease "$tmp/rel.json")"

say "Checksums: every .sha256 = GitHub's digest of its file"
gh release download "$tag" -R "$REPO" -D "$tmp/sums" -p '*.sha256' -p SHA256SUMS
for f in "${files[@]}"; do
    d=$(digest "$f")
    [[ $d =~ ^[0-9a-f]{64}$ ]] || die "GitHub has no sha256 digest for $f"
    line=$(cat "$tmp/sums/$f.sha256")
    [[ $line == "$d  $f" ]] || die "$f.sha256 says '$line', GitHub's digest is $d"
    echo "  ok  $d  $f"
done
for p in "${stable_pairs[@]}"; do
    read -r a b <<<"$p"
    [[ $(digest "$a") == "$(digest "$b")" ]] || die "$b is not the same bytes as $a"
done

say "SHA256SUMS: all seven files"
for f in "${files[@]}"; do cat "$tmp/sums/$f.sha256"; done >"$tmp/SHA256SUMS"
if [[ $(sort "$tmp/sums/SHA256SUMS") == "$(sort "$tmp/SHA256SUMS")" ]]; then
    echo "  complete"
elif [[ $draft != true ]]; then
    die "SHA256SUMS on the public release is incomplete or wrong; fix it by hand (expected:
$(cat "$tmp/SHA256SUMS"))"
elif ((dry)); then
    echo "  would replace SHA256SUMS (draft) with:"; sed 's/^/    /' "$tmp/SHA256SUMS"
else
    gh release upload "$tag" -R "$REPO" --clobber "$tmp/SHA256SUMS"
    view
    gh release download "$tag" -R "$REPO" -D "$tmp/check" -p SHA256SUMS
    cmp -s "$tmp/check/SHA256SUMS" "$tmp/SHA256SUMS" || die "SHA256SUMS didn't upload as written"
    echo "  replaced (draft) with the seven lines"
fi

# --- 2. Publish, not Latest ----------------------------------------------------------------------
if [[ $draft == true ]]; then
    if ((dry)); then
        say "Dry run: would publish $tag (not Latest), check the links, then mark it Latest. Draft files are not public, so the outside checks can't run yet."
        exit 0
    fi
    say "Publishing $tag, not as Latest yet"
    if ((prerelease)); then
        gh release edit "$tag" -R "$REPO" --draft=false --prerelease --latest=false >/dev/null
    else
        gh release edit "$tag" -R "$REPO" --draft=false --latest=false >/dev/null
    fi
    view
    [[ $(jq -r .isDraft "$tmp/rel.json") == false ]] || die "$tag is still a draft"
fi

# --- 3. Every versioned URL from outside --------------------------------------------------------
say "Every file, from outside (HEAD, expect 200)"
urls=("https://github.com/$REPO/releases/tag/$tag")
for f in "${expected[@]}"; do urls+=("$dl/$f"); done
heads_ok "${urls[@]}" || die "some files don't answer 200. $tag is public but NOT Latest, and nothing links to it yet. Look, fix, run this again."

# --- 4. Latest ----------------------------------------------------------------------------------
if ((prerelease)); then
    say "A prerelease: not marked Latest, no latest.json."
    exit 0
fi
latest_tag() { curl -sSI -o /dev/null -w '%{redirect_url}' --max-time 30 "https://github.com/$REPO/releases/latest" | sed 's|.*/releases/tag/||'; }
if [[ $(latest_tag) != "$tag" ]]; then
    if ((dry)); then
        say "Dry run: would mark $tag Latest (now: $(latest_tag))"
        exit 0
    fi
    say "Marking $tag Latest"
    gh release edit "$tag" -R "$REPO" --latest >/dev/null
    # GitHub's redirect can lag the API by a few minutes (0.4.1: 15 s, 0.4.2: over 2 min).
    for _ in $(seq 1 60); do [[ $(latest_tag) == "$tag" ]] && break; sleep 5; done
fi
[[ $(latest_tag) == "$tag" ]] || die "releases/latest is $(latest_tag), not $tag"
say "The stable names lead to $tag"
ok=1 urls=()
for p in "${stable_pairs[@]}"; do
    read -r _ b <<<"$p"
    for f in "$b" "$b.sha256"; do
        hop=$(curl -sSI -o /dev/null -w '%{redirect_url}' --max-time 30 "https://github.com/$REPO/releases/latest/download/$f")
        [[ $hop == "$dl/$f" ]] || { echo "  latest/download/$f goes to '$hop'"; ok=0; continue; }
        urls+=("https://github.com/$REPO/releases/latest/download/$f")
    done
done
((${#urls[@]} == 0)) || heads_ok "${urls[@]}" || ok=0
((ok)) || die "a stable name doesn't lead to $tag's file"

# --- 5. latest.json -----------------------------------------------------------------------------
say "latest.json (versioned links only)"
out=${CARGO_TARGET_DIR:-target}/release-$release
((dry)) && out=$tmp    # a dry run writes nothing outside its temp folder
mkdir -p "$out"
min_supported=$(git show "$tag:packaging/MINIMUM_SUPPORTED" | tr -d '[:space:]')
jq -n --arg version "$version" --argjson build "$build" --arg date "$(git log -1 --format=%cs "$tag")" \
      --arg min "$min_supported" --arg notes "https://github.com/$REPO/releases/tag/$tag" \
      --arg dmg "$dl/Kotora-$release.dmg" --arg src "$dl/kotora-$release.tar.gz" \
      --arg win "$dl/Kotora-$release-windows-x64-setup.exe" --arg win_sha "$(digest "Kotora-$release-windows-x64-setup.exe")" '
    { version: $version, build: $build, date: $date, minimum_supported: $min, notes_url: $notes,
      downloads: { macos_dmg: $dmg, linux_tarball: $src, aur: "https://aur.archlinux.org/packages/kotora",
                   windows_installer: $win, windows_installer_sha256: $win_sha } }' >"$out/latest.json"
cat "$out/latest.json"
packaging/check-links.sh "$out/latest.json"
((dry)) && { say "Dry run: nothing was changed."; exit 0; }

cat <<DONE

$(say "$tag is public, Latest, and complete. latest.json is in $out/latest.json")

Now, and only now, the site (kotora.moe):
   cp $out/latest.json ~/work/kotora.moe/site/latest.json   (plus the version flip on the pages, if any)
   cd ~/work/kotora.moe && git commit -am "latest.json: Kotora $release" && git push origin main
Then, after the deploy (a minute; the update Worker's cache up to 5 more):
   packaging/check-links.sh https://kotora.moe/latest.json
   packaging/check-links.sh 'https://updates.kotora.moe/latest.json?repeat=1'
DONE
