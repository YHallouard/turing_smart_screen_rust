#!/bin/bash
# bc250-dashboard installer for SteamOS (Desktop Mode, run as `deck`).
#
#   curl -fsSL https://github.com/YHallouard/turing_smart_screen_rust/releases/latest/download/install.sh -o /tmp/bc250-install.sh && bash /tmp/bc250-install.sh
#
# Idempotent: re-run it any time (e.g. after a SteamOS update reset /etc) to
# repair the install. Everything lands in /home except the udev rule, the only
# step that needs sudo. Updates are then automatic (bc250-update.timer).
set -euo pipefail

REPO="YHallouard/turing_smart_screen_rust"
SHARE_DIR="$HOME/.local/share/bc250-dashboard"
UDEV_RULE="/etc/udev/rules.d/70-turzx-panel.rules"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

if [ "$(id -u)" -eq 0 ]; then
    echo "Run this as your normal user (deck), not with sudo." >&2
    exit 1
fi

say "Downloading the latest bc250-dashboard release"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/update.sh" \
    "https://github.com/$REPO/releases/latest/download/bc250-dashboard-update.sh"
sh "$tmp/update.sh" --force

say "Panel access (udev rule) - needs your sudo password once"
if cmp -s "$SHARE_DIR/steamos/70-turzx-panel.rules" "$UDEV_RULE"; then
    echo "already installed"
else
    # SteamOS ships the deck user without a password, so sudo cannot work yet.
    if passwd -S "$USER" 2>/dev/null | awk '{exit !($2 == "NP")}'; then
        echo "Your account has no password yet; sudo needs one. Choose it now:"
        passwd
    fi
    sudo -v || {
        echo "sudo failed. If you never set a password: run 'passwd', then re-run this installer." >&2
        exit 1
    }
    # /etc is a writable overlay on SteamOS: no need for steamos-readonly.
    sudo install -Dm644 "$SHARE_DIR/steamos/70-turzx-panel.rules" "$UDEV_RULE"
    sudo udevadm control --reload
    sudo udevadm trigger --subsystem-match=tty --attr-match=idVendor=1a86
fi

say "Enabling the dashboard and automatic updates"
systemctl --user daemon-reload
systemctl --user enable bc250-dashboard.service bc250-update.timer
systemctl --user restart bc250-dashboard.service
systemctl --user start bc250-update.timer
# SteamOS autologins `deck` anyway; linger keeps it working if that's turned off.
loginctl enable-linger "$USER" 2>/dev/null || sudo loginctl enable-linger "$USER" || true

say "Done - $(cat "$SHARE_DIR/VERSION")"
echo "The screen starts on every boot and updates itself."
echo "  logs:    journalctl --user -u bc250-dashboard -f"
echo "  config:  ~/.config/bc250-dashboard/config.toml"
if [ ! -e /dev/turzx-panel ]; then
    echo
    echo "Note: the panel is not detected right now - check the USB cable."
fi
