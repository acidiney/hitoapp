FROM rust:1.99-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS plugin-build
WORKDIR /workspace
ARG HITO_PLUGIN_CACHE_ID=hito-plugin-all-debrid-standalone
ARG HITO_ALL_DEBRID_SOURCE_ID
ARG HITO_ALL_DEBRID_INPUT_SHA256
ARG TARGETARCH
ENV CARGO_TARGET_DIR=/build/all-debrid-release-target \
    CARGO_BUILD_JOBS=2 \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_PROFILE_TEST_DEBUG=0 \
    CARGO_PROFILE_RELEASE_DEBUG=0 \
    CARGO_PROFILE_RELEASE_INCREMENTAL=false \
    CARGO_INCREMENTAL=0
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo .cargo
COPY crates crates
COPY apps/server apps/server
COPY apps/desktop apps/desktop
COPY apps/community apps/community
COPY plugins plugins
COPY vendor vendor
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,id=${HITO_PLUGIN_CACHE_ID}-${HITO_ALL_DEBRID_INPUT_SHA256}-${TARGETARCH},target=/build/all-debrid-release-target \
    printf '%s\n' "${HITO_ALL_DEBRID_SOURCE_ID}" | grep -Eq '^[0-9a-f]{40}$' && \
    printf '%s\n' "${HITO_ALL_DEBRID_INPUT_SHA256}" | grep -Eq '^[0-9a-f]{64}$' && \
    cargo build --locked --offline --release -j 2 -p hito-plugin-all-debrid --bin hito-plugin-all-debrid && \
    cp /build/all-debrid-release-target/release/hito-plugin-all-debrid /hito-plugin-all-debrid

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
ARG HITO_ALL_DEBRID_SOURCE_ID
ARG HITO_ALL_DEBRID_INPUT_SHA256
LABEL org.opencontainers.image.revision="${HITO_ALL_DEBRID_SOURCE_ID}" \
      org.hito.product-input-sha256="${HITO_ALL_DEBRID_INPUT_SHA256}"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/* && groupadd --gid 10001 hito-plugin && useradd --uid 10001 --gid 10001 --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin hito-plugin && install -d -o 10001 -g 10001 -m 0700 /run/secrets
COPY --from=plugin-build --chown=0:0 --chmod=0555 /hito-plugin-all-debrid /usr/local/bin/hito-plugin-all-debrid
COPY --chown=0:0 --chmod=0444 LICENSE /usr/local/share/licenses/hito-plugin-all-debrid/LICENSE
COPY --chown=0:0 --chmod=0444 crates/plugin-sdk/LICENSE /usr/local/share/licenses/hito-plugin-sdk/LICENSE
USER 10001:10001
EXPOSE 8097
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=5s --timeout=3s --start-period=5s --retries=3 CMD curl -fsS --noproxy '*' --max-time 2 --output /dev/null http://127.0.0.1:8097/v1/manifest || exit 1
ENTRYPOINT ["/usr/local/bin/hito-plugin-all-debrid"]
CMD ["--provider-token-file", "/run/secrets/all-debrid-provider-token", "--service-token-file", "/run/secrets/all-debrid-service-token", "--receipt-key-file", "/run/secrets/all-debrid-receipt-key", "--catalog-file", "/run/secrets/all-debrid-catalog.json", "--listen", "0.0.0.0:8097"]
