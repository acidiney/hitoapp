#!/bin/sh
set -eu

make fmt-check
make test-rust
make lint
make web-install
make web-lint
make contracts
make test-web
make web-build
make web-audit
make setup-check
make e2e
