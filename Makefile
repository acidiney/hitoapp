# Single task entry point. Every target wraps the documented Docker Compose commands.
# Isolated runs: make COMPOSE_PROJECT=hito-arch COMPOSE_EXTRA="-f path/override.yaml" test-rust

COMPOSE_PROJECT ?=
COMPOSE_EXTRA ?=
RUST_FEATURES := hito-debrid-real-debrid/test-support,hito-debrid-all-debrid/test-support,hito-debrid-torbox/test-support,hito-debrid-premiumize/test-support,hito-plugin-torbox/test-support,hito-plugin-premiumize/test-support

ifneq ($(COMPOSE_PROJECT),)
export COMPOSE_PROJECT_NAME := $(COMPOSE_PROJECT)
endif

COMPOSE := docker compose -f compose.yaml $(COMPOSE_EXTRA)
RUST := $(COMPOSE) --profile tools run --rm rust
NODE := $(COMPOSE) --profile tools run --rm node

.PHONY: help up down build-tools fmt test-rust test-web lint contracts web-build setup-check e2e desktop verify

help:
	@echo "Targets: up down build-tools fmt test-rust test-web lint contracts web-build setup-check e2e desktop verify"
	@echo "Variables: COMPOSE_PROJECT=<name> COMPOSE_EXTRA='-f extra.yaml'"

up:
	$(COMPOSE) up --build -d server

down:
	$(COMPOSE) down

build-tools:
	$(COMPOSE) --profile tools build rust

fmt:
	$(RUST) cargo fmt --all --check

test-rust:
	$(RUST) cargo test --workspace --locked --features $(RUST_FEATURES)

test-web:
	$(NODE) npm test

lint:
	$(RUST) cargo clippy --workspace --all-targets --locked --features $(RUST_FEATURES) -- -D warnings

contracts:
	./scripts/check-contracts.sh

web-build:
	$(NODE) npm run build

setup-check:
	./scripts/check-setup.sh

e2e:
	$(COMPOSE) --profile e2e rm -sf e2e-server
	$(COMPOSE) --profile e2e up --build -d e2e-server
	$(COMPOSE) --profile e2e run --rm browser; status=$$?; $(COMPOSE) --profile e2e stop e2e-server; exit $$status

desktop:
	./scripts/check-desktop.sh

verify:
	./scripts/verify.sh
