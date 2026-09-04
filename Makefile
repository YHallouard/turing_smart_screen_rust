.PHONY: check fmt clippy test build run run-window run-serial clean \
        release install-udev install enable disable uninstall deb

CARGO ?= cargo
BIN   := bc250-dashboard
USER  := $(shell id -un)
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' daemon/Cargo.toml | head -1)

PREFIX   ?= $(HOME)/.local
BINDIR   := $(PREFIX)/bin
SHAREDIR := $(PREFIX)/share/bc250-dashboard
CONFDIR  := $(HOME)/.config/bc250-dashboard
UNITDIR  := $(HOME)/.config/systemd/user
UDEVRULE := /etc/udev/rules.d/70-turzx-panel.rules

# Fast local gate: formatting, lints, full build.
check: fmt clippy build test

fmt:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

build:
	$(CARGO) build --workspace --all-targets

test:
	$(CARGO) test --workspace

# Render the boot scene to PNG frames in target/frames/.
run:
	$(CARGO) run -p $(BIN) -- --backend png --mode single --scene assets/scenes/boot.toml

# Live preview window (desktop / Mac). Esc or close the window to quit.
run-window:
	$(CARGO) run -p $(BIN) --features window -- --backend window --mode single --loop \
		--scene assets/scenes/boot.toml

# Drive the real panel: boot animation, then the live dashboard.
run-serial:
	$(CARGO) run -p $(BIN) --features serial -- --backend serial

clean:
	$(CARGO) clean

# ---- install (Linux, systemd) -------------------------------------------------

release:
	$(CARGO) build --release --features serial -p $(BIN)

# One-time, needs root: world-rw + stable /dev/turzx-panel symlink for the panel.
install-udev:
	sudo rm -f /etc/udev/rules.d/99-turzx-panel.rules
	sudo install -Dm644 packaging/70-turzx-panel.rules $(UDEVRULE)
	sudo udevadm control --reload
	sudo udevadm trigger --subsystem-match=tty --attr-match=idVendor=1a86
	@# apply to an already-plugged panel without a replug
	-for d in /dev/ttyACM*; do sudo chmod 666 "$$d" 2>/dev/null; done
	@echo ">> udev rule installed."

# Per-user, no root: binary + assets + default config + service unit.
install: release
	install -Dm755 target/release/$(BIN) $(BINDIR)/$(BIN)
	rm -rf $(SHAREDIR) && mkdir -p $(SHAREDIR)
	cp -r assets $(SHAREDIR)/
	install -Dm644 packaging/bc250-dashboard.service $(UNITDIR)/bc250-dashboard.service
	[ -f $(CONFDIR)/config.toml ] || install -Dm644 config.toml $(CONFDIR)/config.toml
	@echo ">> installed under $(PREFIX). Next:  make install-udev   then:  make enable"

# Start now and at boot (linger = no login required).
enable:
	systemctl --user daemon-reload
	systemctl --user enable --now bc250-dashboard.service
	loginctl enable-linger $(USER) || sudo loginctl enable-linger $(USER)
	@echo ">> running. Logs:  journalctl --user -u bc250-dashboard -f"

disable:
	-systemctl --user disable --now bc250-dashboard.service

uninstall: disable
	rm -f $(BINDIR)/$(BIN) $(UNITDIR)/bc250-dashboard.service
	rm -rf $(SHAREDIR)
	systemctl --user daemon-reload
	@echo ">> kept $(CONFDIR) and $(UDEVRULE); remove by hand if wanted."

# ---- .deb ------------------------------------------------------------------
# Hand-built with dpkg-deb (no cargo-deb needed). Output: dist/*.deb
#   sudo apt install ./dist/bc250-dashboard_<ver>_amd64.deb
# The version carries a UTC timestamp + git sha so each `make deb` sorts newer
# and `apt install ./x.deb` upgrades in place (no --reinstall).

GITREV  := $(shell git rev-parse --short=8 HEAD 2>/dev/null || echo nogit)
DEBVER  := $(VERSION)+$(shell date -u +%Y%m%d%H%M%S).$(GITREV)
DEB     := $(BIN)_$(DEBVER)_amd64
DEBROOT := target/deb/$(DEB)

deb: release
	@command -v dpkg-deb >/dev/null || { echo "dpkg-deb not found (apt install dpkg-dev)"; exit 1; }
	rm -rf target/deb dist/$(BIN)_*.deb
	mkdir -p $(DEBROOT)
	install -Dm755 target/release/$(BIN)          $(DEBROOT)/usr/bin/$(BIN)
	mkdir -p                                      $(DEBROOT)/usr/share/bc250-dashboard
	cp -r assets                                  $(DEBROOT)/usr/share/bc250-dashboard/
	install -Dm644 packaging/70-turzx-panel.rules $(DEBROOT)/lib/udev/rules.d/70-turzx-panel.rules
	install -Dm644 config.toml                    $(DEBROOT)/etc/bc250-dashboard/config.toml
	mkdir -p $(DEBROOT)/usr/lib/systemd/user
	sed -e 's|%h/\.local/bin|/usr/bin|g' -e 's|%h/\.local/share|/usr/share|g' \
	    packaging/bc250-dashboard.service > $(DEBROOT)/usr/lib/systemd/user/bc250-dashboard.service
	mkdir -p $(DEBROOT)/DEBIAN
	sed 's|@VERSION@|$(DEBVER)|' packaging/deb/control > $(DEBROOT)/DEBIAN/control
	install -m755 packaging/deb/postinst  $(DEBROOT)/DEBIAN/postinst
	install -m755 packaging/deb/postrm    $(DEBROOT)/DEBIAN/postrm
	install -m644 packaging/deb/conffiles $(DEBROOT)/DEBIAN/conffiles
	mkdir -p dist
	dpkg-deb --root-owner-group --build $(DEBROOT) dist/$(DEB).deb
	@echo ">> dist/$(DEB).deb"
	@echo ">> install:  sudo apt install ./dist/$(DEB).deb"
	@echo ">> then:     systemctl --user enable --now bc250-dashboard"
