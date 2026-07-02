# NextExpress build / test entry points.
#
# Cargo invocations operate on the rust/ workspace so the Makefile lives at the
# repo root regardless of where you run `make` from.

MANIFEST    := --manifest-path rust/Cargo.toml
BIN         := nextexpress
RELEASE_BIN := rust/target/release/$(BIN)
SOURCES     := $(shell find rust/src -name '*.rs') rust/Cargo.toml rust/Cargo.lock
MUTANTS_LOG := target/mutants-run.log
MUTANTS_ARGS ?=
# Base for `make mutants-diff`. Default mutates the working-tree changes vs the
# last commit; override with `make mutants-diff DIFF_BASE=main` for a branch.
DIFF_BASE ?= HEAD
AMIEXPRESS_IMAGE ?= nextexpress/amiexpress-fsuae
AMIEXPRESS_PORT ?= 6023
AMIEXPRESS_AROS_ROMS_VOLUME ?= nextexpress-aros-roms
AMIEXPRESS_AROS_SYSTEM_VOLUME ?= nextexpress-aros-system
AMIEXPRESS_BBS_VOLUME ?= nextexpress-bbs
AMIEXPRESS_DOCKER_ARGS ?=

.PHONY: all build test doctest mutants mutants-diff check fmt clippy clean amiexpress-docker-build amiexpress-docker

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
	cargo nextest run $(MANIFEST)

doctest:
	cargo test $(MANIFEST) --doc

# The FULL sweep — every mutant in the crate (1,800+ as of July 2026,
# ~6-9 hours serial). A scheduled/background job, not a commit gate:
# shard it across parallel runs with MUTANTS_ARGS='--shard k/n'. The
# tee'd log (target/mutants-run.log) plus cargo-mutants' mutants.out
# are the baseline artifacts diff runs are compared against.
mutants:
	mkdir -p rust/target
	cd rust && bash -o pipefail -c 'cargo mutants $(MUTANTS_ARGS) 2>&1 | tee $(MUTANTS_LOG)'

# Mutate only the lines changed since DIFF_BASE. The diff is generated with
# `--relative` from inside rust/ so its paths are crate-relative; a repo-root
# diff makes cargo-mutants report "No mutants to filter".
mutants-diff:
	mkdir -p rust/target
	cd rust && git diff $(DIFF_BASE) --relative > target/mutants.diff && \
		bash -o pipefail -c 'cargo mutants --in-diff target/mutants.diff $(MUTANTS_ARGS) 2>&1 | tee $(MUTANTS_LOG)'

# Mirrors the "Before Committing" checklist in AGENTS.md. The mutation
# step is diff-scoped (uncommitted changes vs DIFF_BASE, default HEAD;
# use DIFF_BASE=main for a branch) — the full sweep takes hours and
# runs as a scheduled job via `make mutants`, never as a commit gate.
check:
	cargo fmt $(MANIFEST) --check
	cargo clippy $(MANIFEST) --all-targets -- -D warnings
	cargo nextest run $(MANIFEST)
	cargo test $(MANIFEST) --doc
	$(MAKE) mutants-diff

fmt:
	cargo fmt $(MANIFEST)

clippy:
	cargo clippy $(MANIFEST) --all-targets -- -D warnings

clean:
	cargo clean $(MANIFEST)
	rm -f $(BIN)

amiexpress-docker-build:
	docker build -f docker/amiexpress-fsuae/Dockerfile -t $(AMIEXPRESS_IMAGE) .

amiexpress-docker: amiexpress-docker-build
	docker volume create $(AMIEXPRESS_AROS_ROMS_VOLUME) >/dev/null
	docker volume create $(AMIEXPRESS_AROS_SYSTEM_VOLUME) >/dev/null
	docker volume create $(AMIEXPRESS_BBS_VOLUME) >/dev/null
	docker run --rm -it \
		-p 127.0.0.1:$(AMIEXPRESS_PORT):6023 \
		-v $(AMIEXPRESS_AROS_ROMS_VOLUME):/opt/aros \
		-v $(AMIEXPRESS_AROS_SYSTEM_VOLUME):/amiga/workbench \
		-v $(AMIEXPRESS_BBS_VOLUME):/amiga/bbs \
		$(AMIEXPRESS_DOCKER_ARGS) \
		$(AMIEXPRESS_IMAGE)
