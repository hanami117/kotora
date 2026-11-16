#!/usr/bin/env bash
# Builds Kotora from this checkout and installs it for the current user (Linux).
#
#   ./install.sh               build and install (or update, after a `git pull`)
#   ./install.sh --uninstall   remove it again (keeps your list, sign-in and settings)
#
# Installs into $PREFIX (default ~/.local):
#   bin/kotora                                               the app (release build, stripped)
#   bin/kot                                                  the command line (stripped)
#   share/applications/io.github.hanami117.kotora.desktop
#   share/icons/hicolor/<size>/apps/io.github.hanami117.kotora.png   (16 to 512 px)
#   share/icons/hicolor/scalable/apps/io.github.hanami117.kotora.svg
# Your settings, list and caches live in ~/.config/kotora, ~/.local/share/kotora and ~/.cache/kotora.
# Adapted from Uzumaki's install.sh.
set -euo pipefail

APP_ID=io.github.hanami117.kotora
PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/kotora"
KOT="$PREFIX/bin/kot"
DESKTOP="$PREFIX/share/applications/$APP_ID.desktop"
ICONS="$PREFIX/share/icons/hicolor"
ICON="$ICONS/scalable/apps/$APP_ID.svg"
EXEC_LINE="Exec=$BIN"
[[ $BIN == *" "* ]] && EXEC_LINE="Exec=\"$BIN\""

cd "$(dirname "$(readlink -f "$0")")"

say() { printf '\033[1;38;2;242;138;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ $(uname -s) == Linux ]] || die "install.sh is for Linux; on macOS build with cargo for now (a DMG comes in v0.2)"

refresh_menus() {
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
    # Only refresh an icon cache that exists: a new one goes stale when other apps add icons.
    [[ -f $ICONS/icon-theme.cache ]] && gtk-update-icon-cache -q -t "$ICONS" 2>/dev/null || true
}

running() { pgrep -xf "$BIN( .*)?" >/dev/null 2>&1; }

uninstall() {
    running && die "Kotora is running. Quit it first (kot quit), then run this again."
    say "Removing Kotora from $PREFIX"
    rm -f "$BIN" "$KOT" "$ICON" "$ICONS"/*/apps/"$APP_ID".png
    # Only remove the menu entry if it is ours (it may point at a build in a source checkout).
    if [[ -f $DESKTOP ]] && grep -qxF "$EXEC_LINE" "$DESKTOP"; then
        rm -f "$DESKTOP"
    elif [[ -f $DESKTOP ]]; then
        say "Kept $DESKTOP: it starts $(grep -m1 '^Exec=' "$DESKTOP" | cut -d= -f2-), not this install"
    fi
    refresh_menus
    say "Done. Your list, sign-in and settings are still in ~/.config/kotora, ~/.local/share/kotora"
    say "and ~/.cache/kotora; delete those folders to forget everything."
}

case "${1:-}" in
    --uninstall) uninstall; exit ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit ;;
    "") ;;
    *) die "unknown option '$1' (try --help)" ;;
esac

# --- Requirements -------------------------------------------------------------------------------
missing=()
for tool in cargo git pkg-config; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
if ((${#missing[@]})); then
    printf 'Missing: %s\n' "${missing[*]}" >&2
    if command -v pacman >/dev/null; then
        echo "On Arch: sudo pacman -S --needed rust git pkgconf libxkbcommon wayland" >&2
    fi
    die "install the tools above, then run this again"
fi
if ! grep -q 'libvulkan\.so\.1' <(ldconfig -p 2>/dev/null); then  # not a pipe: grep -q + pipefail = SIGPIPE
    say "Warning: no Vulkan loader found. Kotora draws with Vulkan; install vulkan-icd-loader and"
    say "your GPU's driver (vulkan-radeon, vulkan-intel or nvidia-utils)."
fi
running && die "Kotora is running from $PREFIX. Quit it first (kot quit), then run this again."

# --- Build --------------------------------------------------------------------------------------
say "Building Kotora (release; the first build takes several minutes)"
# The update check then says "git pull, then ./install.sh" instead of offering a download.
KOTORA_CHANNEL=install-sh cargo build --release --locked -p kotora -p kotora-ctl
target="${CARGO_TARGET_DIR:-target}/release/kotora"
[[ -x $target ]] || die "build finished but $target is missing"
[[ -x ${target%kotora}kot ]] || die "build finished but ${target%kotora}kot is missing"

# --- Install ------------------------------------------------------------------------------------
say "Installing into $PREFIX"
# Strip the installed copies only (about 36 MB -> 30 MB for kotora); the build in target/ keeps its
# symbols for backtraces. `strip` comes with binutils (base-devel on Arch).
strip_flag=()
if command -v strip >/dev/null; then
    strip_flag=(-s)
else
    say "Note: \`strip\` not found (binutils), so the binaries keep their symbols (about 6 MB more)."
fi
install -Dm755 "${strip_flag[@]}" "$target" "$BIN"
install -Dm755 "${strip_flag[@]}" "${target%kotora}kot" "$KOT"
install -Dm644 "assets/icons/$APP_ID.svg" "$ICON"
for icon in assets/icons/hicolor/*/apps/"$APP_ID".png; do
    size=$(basename "$(dirname "$(dirname "$icon")")")
    install -Dm644 "$icon" "$ICONS/$size/apps/$APP_ID.png"
done

if [[ -f $DESKTOP ]] && ! grep -qxF "$EXEC_LINE" "$DESKTOP"; then
    say "Replacing the existing menu entry (it started $(grep -m1 '^Exec=' "$DESKTOP" | cut -d= -f2-))"
fi
mkdir -p "$(dirname "$DESKTOP")"
sed "s|^Exec=.*|$EXEC_LINE|" "packaging/linux/$APP_ID.desktop" > "$DESKTOP"
chmod 644 "$DESKTOP"
refresh_menus

say "Installed. Start Kotora from your app menu, or run: $BIN"
say "From a terminal: kot (status), kot show, kot quit"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) say "Note: $PREFIX/bin is not on your PATH, so plain \`kotora\` won't work in a terminal." ;;
esac
say "To update later: git pull && ./install.sh"
