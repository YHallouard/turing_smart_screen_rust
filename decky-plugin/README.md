# BC-250 Dashboard — Decky Loader plugin

Control panel in Gaming Mode's quick-access menu for the `bc250-dashboard`
daemon (the daemon is **not** inside the plugin: it stays a systemd user
service so the screen works at boot, in Desktop Mode and without Steam).

- `main.py` — backend: runs `bc250-dashboard config json|set`, `systemctl --user`,
  `bc250-dashboard-update`, always as the Steam user.
- `src/index.tsx` — UI: status, update, and a form generated from the daemon's
  own settings schema (`bc250-dashboard config json`), so new config keys only
  need adding in `daemon/src/configctl.rs`.

Build / package: `sh decky-plugin/package.sh [tag] [out-dir]` → `bc250-dashboard-decky.zip`.
Install on the console: Decky > Settings > Developer > *Install plugin from ZIP*.
