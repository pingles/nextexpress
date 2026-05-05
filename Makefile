# NextExpress build / test entry points.
#
# All cargo invocations operate on the rust/ workspace via
# --manifest-path so the Makefile lives at the repo root regardless of
# where you run `make` from.

MANIFEST    := --manifest-path rust/Cargo.toml
BIN         := nextexpress
RELEASE_BIN := rust/target/release/$(BIN)
SOURCES     := $(shell find rust/src -name '*.rs') rust/Cargo.toml rust/Cargo.lock

.PHONY: all build test check fmt clippy clean

all: build

# `build` is a phony alias to the real binary target so callers can
# always say `make build`. The binary itself depends on the Rust
# sources, so make skips the whole cargo invocation when nothing has
# changed.
build: $(BIN)

$(BIN): $(SOURCES)
	cargo build $(MANIFEST) --release
	cp $(RELEASE_BIN) $@

test:
	cargo test $(MANIFEST)

# Mirrors the "Before Committing" checklist in AGENTS.md.
check:
	cargo fmt $(MANIFEST) --check
	cargo clippy $(MANIFEST) --all-targets -- -D warnings
	cargo test $(MANIFEST)

fmt:
	cargo fmt $(MANIFEST)

clippy:
	cargo clippy $(MANIFEST) --all-targets -- -D warnings

clean:
	cargo clean $(MANIFEST)
	rm -f $(BIN)
