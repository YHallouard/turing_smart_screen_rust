"""BC-250 Dashboard — Decky Loader backend.

A thin layer over the `bc250-dashboard` binary: the daemon keeps running as a
systemd *user* service (so the panel works with Steam closed, in Desktop Mode,
at boot…); this plugin only configures, restarts and updates it.

Every command is run as the Steam user (Decky may run us as root) with that
user's runtime dir, so `systemctl --user` reaches the right session bus.
"""

import asyncio
import json
import os
import pwd

import decky

UNIT = "bc250-dashboard.service"
REPO = "YHallouard/turing_smart_screen_rust"


def _user() -> pwd.struct_passwd:
    name = getattr(decky, "DECKY_USER", None) or "deck"
    try:
        return pwd.getpwnam(name)
    except KeyError:
        return pwd.getpwuid(os.getuid())


def _env(u: pwd.struct_passwd) -> dict:
    env = dict(os.environ)
    env.update(
        HOME=u.pw_dir,
        USER=u.pw_name,
        XDG_RUNTIME_DIR=f"/run/user/{u.pw_uid}",
        DBUS_SESSION_BUS_ADDRESS=f"unix:path=/run/user/{u.pw_uid}/bus",
        PATH=f"{u.pw_dir}/.local/bin:" + env.get("PATH", "/usr/bin:/bin"),
    )
    env.pop("LD_LIBRARY_PATH", None)  # Steam's runtime libs break system tools
    return env


async def _run(*argv: str, timeout: float = 120.0) -> tuple[int, str]:
    """Run argv as the Steam user. Returns (exit code, stdout+stderr)."""
    u = _user()
    cmd = list(argv)
    if os.geteuid() == 0 and u.pw_uid != 0:
        cmd = ["runuser", "-u", u.pw_name, "--", *cmd]
    try:
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            env=_env(u),
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.STDOUT,
        )
        out, _ = await asyncio.wait_for(proc.communicate(), timeout)
        return proc.returncode or 0, out.decode(errors="replace").strip()
    except asyncio.TimeoutError:
        proc.kill()
        return 124, "timed out"
    except FileNotFoundError as e:
        return 127, str(e)


def _bin() -> str:
    return os.path.join(_user().pw_dir, ".local", "bin", "bc250-dashboard")


def _installed_version() -> str:
    path = os.path.join(
        _user().pw_dir, ".local", "share", "bc250-dashboard", "VERSION"
    )
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError:
        return ""


class Plugin:
    async def get_status(self) -> dict:
        """Service state, installed version and panel presence."""
        _, active = await _run("systemctl", "--user", "is-active", UNIT)
        _, enabled = await _run("systemctl", "--user", "is-enabled", UNIT)
        return {
            "installed": os.path.exists(_bin()),
            "version": _installed_version(),
            "active": active == "active",
            "state": active,
            "enabled": enabled == "enabled",
            "panel": os.path.exists("/dev/turzx-panel"),
        }

    async def get_config(self) -> dict:
        """Settings schema + current values (`bc250-dashboard config json`)."""
        code, out = await _run(_bin(), "config", "json")
        if code != 0:
            return {"error": out or f"exit {code}", "settings": []}
        try:
            return json.loads(out)
        except json.JSONDecodeError as e:
            return {"error": f"bad JSON from daemon: {e}", "settings": []}

    async def set_config(self, changes: dict) -> dict:
        """Apply {key: value}; validated by the daemon, then one restart."""
        for key, val in changes.items():
            if isinstance(val, bool):
                val = "true" if val else "false"
            code, out = await _run(
                _bin(), "config", "set", "--no-restart", key, str(val)
            )
            if code != 0:
                return {"ok": False, "error": out}
        code, out = await _run("systemctl", "--user", "restart", UNIT)
        return {"ok": code == 0, "error": "" if code == 0 else out}

    async def restart(self) -> dict:
        code, out = await _run("systemctl", "--user", "restart", UNIT)
        return {"ok": code == 0, "error": out}

    async def latest_version(self) -> dict:
        """Latest release tag via the /releases/latest redirect (no API quota)."""
        code, out = await _run(
            "curl", "-fsSLI", "-o", "/dev/null", "-w", "%{url_effective}",
            f"https://github.com/{REPO}/releases/latest",
            timeout=20,
        )
        tag = out.rsplit("/", 1)[-1] if code == 0 else ""
        return {"tag": tag if tag.startswith("v") else "", "error": "" if code == 0 else out}

    async def update(self) -> dict:
        """Run the installed updater (checksum-verified, restarts the service)."""
        code, out = await _run("bc250-dashboard-update", timeout=300)
        return {"ok": code == 0, "log": out, "version": _installed_version()}

    async def logs(self, lines: int = 40) -> str:
        _, out = await _run(
            "journalctl", "--user", "-u", UNIT.removesuffix(".service"),
            "-n", str(int(lines)), "--no-pager", "-o", "cat",
        )
        return out

    async def _main(self):
        decky.logger.info("bc250-dashboard plugin loaded")

    async def _unload(self):
        decky.logger.info("bc250-dashboard plugin unloaded")
