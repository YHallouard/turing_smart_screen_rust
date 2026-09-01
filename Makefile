.PHONY: check fmt clippy test build run run-window run-serial clean

# Fast local gate: formatting, lints, full build.
check: fmt clippy build test

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

build:
	cargo build --workspace --all-targets

test:
	cargo test --workspace

# Render the boot scene to PNG frames in target/frames/.
run:
	cargo run -p bc250-dashboard -- --backend png

# Live preview window (desktop / Mac). Esc or close the window to quit.
run-window:
	cargo run -p bc250-dashboard --features window -- --backend window --loop

# Drive the real panel over USB serial.
run-serial:
	cargo run -p bc250-dashboard --features serial -- --backend serial

clean:
	cargo clean
