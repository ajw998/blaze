SHELL := /bin/bash

CARGO ?= cargo
WORKSPACE_FLAGS ?= --workspace
BIN_DIR := target
RELEASE_BIN := $(BIN_DIR)/release/blaze
DAEMON_ARGS ?=

.PHONY: build build-release daemon test format benchmark help

help:
	@echo "make build          - Build all workspace binaries (debug)"
	@echo "make build-release  - Build all workspace binaries (release)"
	@echo "make daemon         - Run the blaze daemon locally"
	@echo "make test           - Run unit/integration tests"
	@echo "make format         - Run rustfmt across the workspace"
	@echo "make benchmark      - Run scripts/benchmark.sh (requires release build)"

build:
	$(CARGO) build $(WORKSPACE_FLAGS) --all-targets

build-release:
	$(CARGO) build $(WORKSPACE_FLAGS) --all-targets --release

daemon: build
	$(CARGO) run -p blaze-daemon -- $(DAEMON_ARGS)

test:
	$(CARGO) test $(WORKSPACE_FLAGS) --all-targets

format:
	$(CARGO) fmt $(WORKSPACE_FLAGS)

benchmark: build-release
	BLAZE_BIN=$(RELEASE_BIN) scripts/benchmark.sh
