COMPOSE_PROJECT ?=
COMPOSE_EXTRA ?=

ifneq ($(COMPOSE_PROJECT),)
export COMPOSE_PROJECT_NAME := $(COMPOSE_PROJECT)
endif
export COMPOSE_FILE := compose.yaml$(if $(COMPOSE_EXTRA),:$(COMPOSE_EXTRA))

COMPOSE := docker compose
RUST := $(COMPOSE) --profile tools run --rm rust
NODE := $(COMPOSE) --profile tools run --rm node

.PHONY: help up down build-tools fmt fmt-check test-rust test-web web-install web-audit lint web-lint contracts web-build setup-check e2e desktop verify

help:
	@echo "Targets that act on the running 'hito' application project: up down"
	@echo "Targets that run one-off tool containers: build-tools fmt fmt-check test-rust test-web web-install web-audit lint web-lint contracts web-build setup-check e2e desktop verify"
	@echo "Variables: COMPOSE_PROJECT=<name> redirects every target except up and down"
	@echo "           COMPOSE_EXTRA=<override.yaml[:more.yaml]> is added to the Compose file list for every target"

up:
	COMPOSE_PROJECT_NAME=hito $(COMPOSE) up --build -d server

down:
	COMPOSE_PROJECT_NAME=hito $(COMPOSE) down

build-tools:
	$(COMPOSE) --profile tools build rust

fmt:
	$(RUST) cargo fmt --all

fmt-check:
	$(RUST) cargo fmt --all --check

test-rust:
	$(RUST) cargo test --workspace --locked
	$(COMPOSE) --profile tools run --rm -e 'RUSTFLAGS=--cfg rustix_use_libc' rust cargo test --locked -p hito-core --test storage_walk_stalls -- --ignored

test-web:
	$(NODE) npm test

web-install:
	$(NODE) npm ci

web-audit:
	$(NODE) npm audit --audit-level=high

lint:
	$(RUST) cargo clippy --workspace --all-targets --locked -- -D warnings

web-lint:
	$(NODE) npm run lint

contracts:
	./scripts/check-contracts.sh

web-build:
	$(NODE) npm run build

setup-check:
	./scripts/check-setup.sh

e2e:
	$(NODE) node /workspace/scripts/audio-fixtures.mjs --e2e
	$(COMPOSE) --profile e2e rm -sf e2e-server
	$(COMPOSE) --profile e2e up --build -d e2e-server
	$(COMPOSE) --profile e2e run --rm browser; status=$$?; $(COMPOSE) --profile e2e stop e2e-server; exit $$status

desktop:
	./scripts/check-desktop.sh

verify:
	./scripts/verify.sh
