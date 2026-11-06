COMPOSE_PROJECT ?=
COMPOSE_EXTRA ?=
E2E_SCRIPT ?= e2e

ifneq ($(COMPOSE_PROJECT),)
export COMPOSE_PROJECT_NAME := $(COMPOSE_PROJECT)
endif
export COMPOSE_FILE := compose.yaml$(if $(COMPOSE_EXTRA),:$(COMPOSE_EXTRA))

COMPOSE := docker compose
RUST := $(COMPOSE) --profile tools run --rm rust
NODE := $(COMPOSE) --profile tools run --rm node

.PHONY: help up down build-tools fmt fmt-check test-rust test-web check-core check-server check-playback check-plugins check-community check-web web-install web-audit lint web-lint contracts web-build setup-check e2e desktop verify bench-playback

help:
	@echo "Targets that act on the running 'hito' application project: up down"
	@echo "Targets that run one-off tool containers: build-tools fmt fmt-check test-rust test-web web-install web-audit lint web-lint contracts web-build setup-check e2e desktop verify"
	@echo "Opt-in measurement (about 80 minutes, see docs/performance.md): bench-playback"
	@echo "Area checks (tests for one area, through the tools profile): check-core check-server check-playback check-plugins check-community check-web"
	@echo "Variables: COMPOSE_PROJECT=<name> redirects every target except up and down"
	@echo "           COMPOSE_EXTRA=<override.yaml[:more.yaml]> is added to the Compose file list for every target"
	@echo "           E2E_SCRIPT=<npm script> selects the Playwright run of 'make e2e' (default e2e; see apps/web/package.json)"

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

CORE_PACKAGES := -p hito-core -p hito-audio -p hito-testkit -p hito-metadata-musicbrainz -p hito-remote-audio
SERVER_PACKAGES := -p hito-server
PLUGIN_PACKAGES := -p hito-plugin-sdk -p hito-plugin-folder -p hito-plugin-spotify -p hito-plugin-youtube -p hito-plugin-jamendo -p hito-plugin-listenbrainz -p hito-plugin-real-debrid -p hito-plugin-all-debrid -p hito-plugin-torbox -p hito-plugin-premiumize -p hito-debrid-real-debrid -p hito-debrid-all-debrid -p hito-debrid-torbox -p hito-debrid-premiumize

check-core:
	$(RUST) cargo test --locked $(CORE_PACKAGES)
	$(COMPOSE) --profile tools run --rm -e 'RUSTFLAGS=--cfg rustix_use_libc' rust cargo test --locked -p hito-core --test storage_walk_stalls -- --ignored

check-server:
	$(RUST) cargo test --locked $(SERVER_PACKAGES)

check-playback:
	$(RUST) cargo test --locked -p hito-playback

check-plugins:
	$(RUST) cargo test --locked $(PLUGIN_PACKAGES)

check-community:
	$(RUST) cargo test --locked -p hito-community

check-web:
	$(MAKE) test-web
	$(MAKE) web-lint

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
	@printf '%s\n' '$(E2E_SCRIPT)' | grep -Eq '^[a-z0-9:_-]+$$' || { echo 'E2E_SCRIPT must match ^[a-z0-9:_-]+$$' >&2; exit 2; }
	$(COMPOSE) --profile e2e run --rm browser npm run '$(E2E_SCRIPT)'; status=$$?; $(COMPOSE) --profile e2e stop e2e-server; exit $$status

desktop:
	./scripts/check-desktop.sh

verify:
	./scripts/verify.sh

bench-playback:
	sh scripts/bench-playback.sh
