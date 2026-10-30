# One image recipe for every Hito plugin.
#   docker build -f docker/plugin.Dockerfile --build-arg PLUGIN=hito-plugin-spotify --build-arg PORT=8092 .
# PLUGIN is the Cargo package and binary name. PORT is the port the plugin listens on; pass the same
# value in the container command (--listen 0.0.0.0:PORT). EXTRAS selects optional runtime files:
# none (default), folder (generated demo audio) or youtube (pinned yt-dlp and Deno).
ARG EXTRAS=none

FROM python:3.13-slim@sha256:70729b46c69b4f1e97c4822c1af3df53a1476cf5ddc6c087c0c10bc3a5678c2f AS folder-demo-audio
COPY scripts/plugins/generate-demo.py /generate-demo.py
RUN python /generate-demo.py /demo

FROM scratch AS extras-folder
COPY --from=folder-demo-audio /demo /demo

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS youtube-runtime
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl unzip && rm -rf /var/lib/apt/lists/*
COPY scripts/plugins/youtube/install-runtime.sh /install-runtime.sh
RUN sh /install-runtime.sh "$TARGETARCH" /runtime

FROM scratch AS extras-youtube
COPY --from=youtube-runtime /runtime/ /

FROM scratch AS extras-none

FROM rust:1.99-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS plugin-build
ARG PLUGIN
ARG SOURCE_ID=
ARG INPUT_SHA256=
ARG HITO_PLUGIN_CACHE_ID=hito-plugin-build
ARG CARGO_OFFLINE=0
ARG TARGETARCH
WORKDIR /workspace
ENV CARGO_TARGET_DIR=/build/plugin-release-target \
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
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=${HITO_PLUGIN_CACHE_ID}-${SOURCE_ID}-${INPUT_SHA256}-${TARGETARCH},sharing=locked,target=/build/plugin-release-target \
    test -n "${PLUGIN}" && \
    { [ -z "${SOURCE_ID}" ] || printf '%s\n' "${SOURCE_ID}" | grep -Eq '^[0-9a-f]{40}$'; } && \
    { [ -z "${INPUT_SHA256}" ] || printf '%s\n' "${INPUT_SHA256}" | grep -Eq '^[0-9a-f]{64}$'; } && \
    printf '%s\n' "${TARGETARCH}" | grep -Eq '^[a-z0-9][a-z0-9_]*$' && \
    { [ "${CARGO_OFFLINE}" != 1 ] || offline=--offline; } && \
    cargo build --locked ${offline:-} --release -j 2 -p "${PLUGIN}" --bin "${PLUGIN}" && \
    cp "/build/plugin-release-target/release/${PLUGIN}" /plugin

FROM extras-${EXTRAS} AS extras

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS runtime
ARG PLUGIN
ARG PORT
ARG STATE_DIR=/state
ARG SOURCE_ID=
ARG INPUT_SHA256=
ARG TARGETARCH
LABEL org.hito.plugin="${PLUGIN}" \
      org.opencontainers.image.revision="${SOURCE_ID}" \
      org.hito.product-input-sha256="${INPUT_SHA256}" \
      org.hito.build-target-arch="${TARGETARCH}"
RUN test -n "${PORT}" && \
    apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 hito-plugin && \
    useradd --uid 10001 --gid 10001 --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin hito-plugin && \
    install -d -o 10001 -g 10001 -m 0700 /run/secrets "${STATE_DIR}"
COPY --from=extras / /
COPY --from=plugin-build --chown=0:0 --chmod=0555 /plugin /usr/local/bin/hito-plugin
COPY --chown=0:0 --chmod=0444 LICENSE /usr/local/share/licenses/${PLUGIN}/LICENSE
COPY --chown=0:0 --chmod=0444 crates/plugin-sdk/LICENSE /usr/local/share/licenses/hito-plugin-sdk/LICENSE
USER 10001:10001
ENV HITO_PLUGIN_PORT=${PORT}
EXPOSE ${PORT}
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=5s --timeout=3s --start-period=5s --retries=3 CMD curl -fsS --noproxy '*' --max-time 2 --output /dev/null http://127.0.0.1:${HITO_PLUGIN_PORT}/v1/manifest || exit 1
ENTRYPOINT ["/usr/local/bin/hito-plugin"]
