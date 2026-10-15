#!/bin/sh
# bc250-dashboard updater (SteamOS / any systemd distro, per-user, no root).
#
# Installed as ~/.local/bin/bc250-dashboard-update and run by
# bc250-update.timer. Fetches the latest GitHub release, verifies its
# checksum, and swaps it in under ~/.local (survives SteamOS OS updates).
#
#   bc250-dashboard-update            update if a newer release exists
#   bc250-dashboard-update --force    reinstall the latest release
#
# Never touches ~/.config/bc250-dashboard/config.toml once it exists.
set -eu

REPO="YHallouard/turing_smart_screen_rust"
ASSET="bc250-dashboard-x86_64-linux.tar.gz"
BIN_DIR="$HOME/.local/bin"
SHARE_DIR="$HOME/.local/share/bc250-dashboard"
CONF_DIR="$HOME/.config/bc250-dashboard"
UNIT_DIR="$HOME/.config/systemd/user"
UDEV_RULE="/etc/udev/rules.d/70-turzx-panel.rules"

FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1

log() { echo "bc250-update: $*"; }

# One updater at a time (timer, manual run and install.sh may overlap).
mkdir -p "$SHARE_DIR"
exec 9>"$SHARE_DIR/../.bc250-update.lock"
flock -n 9 || { log "another update is running, skipping"; exit 0; }

# Latest tag via the /releases/latest redirect (no API call, no rate limit).
latest_url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
    "https://github.com/$REPO/releases/latest" 2>/dev/null) || {
    log "offline or GitHub unreachable, skipping"
    exit 0
}
tag=${latest_url##*/}
case "$tag" in
    v*) ;;
    *) log "no published release yet"; exit 0 ;;
esac

current=$(cat "$SHARE_DIR/VERSION" 2>/dev/null || echo none)
if [ "$tag" = "$current" ] && [ "$FORCE" -eq 0 ]; then
    log "up to date ($current)"
    exit 0
fi
log "installing $tag (was $current)"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
base="https://github.com/$REPO/releases/download/$tag"
curl -fsSL -o "$tmp/$ASSET" "$base/$ASSET"
curl -fsSL -o "$tmp/$ASSET.sha256" "$base/$ASSET.sha256"
(cd "$tmp" && sha256sum -c --quiet "$ASSET.sha256")
tar -xzf "$tmp/$ASSET" -C "$tmp"
pkg="$tmp/bc250-dashboard"

# Refuse a release whose binary doesn't even start here (e.g. missing libs).
"$pkg/bin/bc250-dashboard" --version >/dev/null || {
    log "new binary fails to run on this system, keeping $current"
    exit 1
}

# Binaries: write next to the target then rename, so a running process keeps
# its old inode and nothing ever sees a half-written file.
mkdir -p "$BIN_DIR"
install -m755 "$pkg/bin/bc250-dashboard" "$BIN_DIR/.bc250-dashboard.new"
mv -f "$BIN_DIR/.bc250-dashboard.new" "$BIN_DIR/bc250-dashboard"
install -m755 "$pkg/steamos/update.sh" "$BIN_DIR/.bc250-dashboard-update.new"
mv -f "$BIN_DIR/.bc250-dashboard-update.new" "$BIN_DIR/bc250-dashboard-update"

# Shared data (assets, unit files, udev rule, VERSION): swap the whole dir.
rm -rf "$SHARE_DIR.new" "$SHARE_DIR.old"
mkdir -p "$SHARE_DIR.new"
cp -r "$pkg/assets" "$pkg/steamos" "$pkg/config.toml" "$pkg/VERSION" "$SHARE_DIR.new/"
[ -d "$SHARE_DIR" ] && mv "$SHARE_DIR" "$SHARE_DIR.old"
mv "$SHARE_DIR.new" "$SHARE_DIR"
rm -rf "$SHARE_DIR.old"

# Default config only on first install.
if [ ! -f "$CONF_DIR/config.toml" ]; then
    install -Dm644 "$pkg/config.toml" "$CONF_DIR/config.toml"
fi

mkdir -p "$UNIT_DIR"
for u in bc250-dashboard.service bc250-update.service bc250-update.timer; do
    install -m644 "$pkg/steamos/$u" "$UNIT_DIR/$u"
done

# (--force = called by install.sh, which installs the rule right after.)
if [ "$FORCE" -eq 0 ] && ! cmp -s "$pkg/steamos/70-turzx-panel.rules" "$UDEV_RULE"; then
    log "WARNING: udev rule missing or outdated (e.g. after a SteamOS update);"
    log "         re-run the installer once to restore it."
fi

if [ -n "${XDG_RUNTIME_DIR:-}" ]; then
    systemctl --user daemon-reload
    if systemctl --user is-active --quiet bc250-dashboard.service ||
       systemctl --user is-enabled --quiet bc250-dashboard.service; then
        systemctl --user restart bc250-dashboard.service
    fi
else
    log "no user session bus; the new version starts on next boot"
fi
log "now on $tag"
