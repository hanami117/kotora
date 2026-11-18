#!/usr/bin/env bash
# Checks a latest.json before (and after) it goes live: every link in it must answer 200 now.
#
#   packaging/check-links.sh target/release-0.5.0/latest.json     a file (before you copy it to the site)
#   packaging/check-links.sh https://kotora.moe/latest.json       a live one (after the deploy)
#   packaging/check-links.sh https://updates.kotora.moe/latest.json?repeat=1
#
# It fails (exit 1) when:
#   - the file doesn't parse, or has no version;
#   - a link points to github.com/<repo>/releases/latest (…/latest/download/… or the page): latest.json
#     names one version, so its links name that version's tag (releases/download/v<release>/…), never
#     whatever release is Latest at the moment the user clicks;
#   - a GitHub release link names another tag than v<version> or v<version>-<build>;
#   - a link (notes_url and every web link in downloads) doesn't answer 200 to a HEAD, redirects followed;
#   - windows_installer_sha256 isn't the sha256 in the installer's own .sha256 file on the release.
# Nothing is changed anywhere: it only reads.
set -euo pipefail

die() { printf 'check-links.sh: %s\n' "$*" >&2; exit 1; }
command -v jq >/dev/null || die "needs jq"
command -v curl >/dev/null || die "needs curl"
(($# == 1)) || die "usage: packaging/check-links.sh <latest.json file or URL>"

src=$1
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
if [[ $src =~ ^https?:// ]]; then
    curl -fsSL --max-time 30 -o "$tmp/latest.json" "$src" || die "can't fetch $src"
else
    [[ -f $src ]] || die "no file $src"
    cp "$src" "$tmp/latest.json"
fi
j=$tmp/latest.json
jq -e 'type == "object"' "$j" >/dev/null 2>&1 || die "$src is not a JSON object"

version=$(jq -r '.version // empty' "$j")
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "bad or no version: '$version'"
build=$(jq -r '.build // 1' "$j")
if [[ $build == 1 ]]; then release=$version; else release=$version-$build; fi
tag=v$release

# HEAD with redirects followed. GitHub's release assets answer HEAD (302 to their storage, then 200).
# A few tries: a file uploaded a minute ago can take a moment to reach every edge.
http_ok() {
    local code i
    for i in 1 2 3; do
        code=$(curl -sSIL -o /dev/null -w '%{http_code}' --max-time 30 "$1" 2>/dev/null) || code=000
        [[ $code == 200 ]] && return 0
        ((i < 3)) && sleep 5
    done
    echo "$code"
    return 1
}

bad=0
fail() { printf '  FAIL  %s\n' "$*"; bad=1; }

echo "latest.json: $src"
echo "version $version, build $build (tag $tag)"
while IFS=$'\t' read -r key url; do
    [[ $url =~ ^https?:// ]] || continue
    if [[ $url =~ ^https://github\.com/[^/]+/[^/]+/releases/latest(/|$) ]]; then
        fail "$key: $url points to whatever release is Latest; use releases/download/$tag/<file>"
        continue
    fi
    if [[ $url =~ ^https://github\.com/[^/]+/[^/]+/releases/(download|tag)/([^/]+) ]] && [[ ${BASH_REMATCH[2]} != "$tag" ]]; then
        fail "$key: $url names tag ${BASH_REMATCH[2]}, not $tag"
        continue
    fi
    if code=$(http_ok "$url"); then
        printf '  200   %s  %s\n' "$key" "$url"
    else
        fail "$key: HTTP $code  $url"
    fi
done < <(jq -r '[["notes_url", .notes_url]] + ((.downloads // {}) | to_entries | map([.key, .value]))
               | .[] | select(.[1] | type == "string") | @tsv' "$j")

win=$(jq -r '.downloads.windows_installer // empty' "$j")
win_sha=$(jq -r '.downloads.windows_installer_sha256 // empty' "$j")
if [[ -n $win || -n $win_sha ]]; then
    [[ -n $win && $win_sha =~ ^[0-9a-f]{64}$ ]] || fail "windows_installer and windows_installer_sha256 go together (64 hex digits)"
    if [[ -n $win && $win_sha =~ ^[0-9a-f]{64}$ ]]; then
        have=$(curl -fsSL --max-time 30 "$win.sha256" 2>/dev/null | awk 'NR == 1 { print $1 }') || have=
        if [[ $have == "$win_sha" ]]; then
            printf '  sha   windows_installer_sha256 = %s.sha256\n' "${win##*/}"
        else
            fail "windows_installer_sha256 $win_sha != ${have:-nothing} in $win.sha256"
        fi
    fi
fi

((bad == 0)) || die "some links are not ready: don't publish this latest.json"
echo "All links answer 200."
