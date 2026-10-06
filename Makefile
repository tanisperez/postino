.DEFAULT_GOAL := help

.PHONY: help run build release dist check lint lint-fix format format-check test clean screenshots docs site sample-server sample

help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "  run           Build and run the app (debug)"
	@echo "  build         Build in debug mode"
	@echo "  release       Build an optimized release binary"
	@echo "  dist          Build the release packages for this OS into target/dist"
	@echo "  check         Fast type-check without producing a binary"
	@echo "  lint          Run Clippy (warnings are errors)"
	@echo "  lint-fix      Run Clippy and apply automatic fixes"
	@echo "  format        Format all Rust code with rustfmt"
	@echo "  format-check  Check formatting without modifying files"
	@echo "  test          Run all tests"
	@echo "  clean         Remove build artifacts"
	@echo "  screenshots   Regenerate the website screenshots (site/img)"
	@echo "  docs          Build the user documentation (site/docs-src into site/docs)"
	@echo "  site          Build the documentation and serve the website on http://localhost:8000"
	@echo "  sample-server Start the local server of the sample suite (HTTP 8080, HTTPS 8443 to 8447)"
	@echo "  sample        Start that server and open the app on samples/workspace"

run:
	cargo run

build:
	cargo build

release:
	cargo build --release

# The workspace version from Cargo.toml, for the package names.
VERSION := $(shell sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)

# Linux needs nfpm on PATH, macOS the Xcode command line tools. On Windows, compile
# packaging/windows/postino.iss with Inno Setup (see its README).
dist: release
	@case "$$(uname -s)" in \
	Linux) packaging/linux/package.sh $(VERSION) target/release/postino target/dist ;; \
	Darwin) packaging/macos/bundle.sh $(VERSION) target/release/postino target/dist ;; \
	*) echo "make dist supports Linux and macOS; see packaging/windows/README.md"; exit 1 ;; \
	esac

check:
	cargo check --all-targets

lint:
	cargo clippy --all-targets -- -D warnings

lint-fix:
	cargo clippy --all-targets --fix --allow-dirty --allow-staged

format:
	cargo fmt --all

format-check:
	cargo fmt --all -- --check

test:
	cargo test --all-targets

clean:
	cargo clean

# The docs build and the sample suite need Python 3 and nothing else (see docs/sample-suite.md).
PYTHON ?= python3

screenshots:
	site/screenshots/capture.sh

# site/docs is generated, never edited by hand (see site/README.md).
docs:
	$(PYTHON) site/docs-src/build.py

site: docs
	$(PYTHON) -m http.server -d site 8000

sample-server:
	$(PYTHON) samples/server/server.py

sample:
	@$(PYTHON) samples/server/server.py > /dev/null & pid=$$!; \
	trap 'kill $$pid' EXIT INT TERM; \
	cargo run -- samples/workspace
