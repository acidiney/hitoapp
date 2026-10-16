FROM rust:1.99-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS rust-tools
WORKDIR /workspace
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg && rm -rf /var/lib/apt/lists/*
RUN rustup component add rustfmt clippy rust-src rust-analyzer
ENV CARGO_TARGET_DIR=/build/target

FROM node:22-bookworm-slim@sha256:c3de60bf2f9dd0ac6370e6117950ff62d6e339527e7472301c9c78a017978392 AS web-build
WORKDIR /web
COPY apps/web/package*.json ./
RUN npm ci
COPY apps/web/ ./
RUN npm run build

FROM rust-tools AS server-build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo .cargo
COPY crates crates
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target cargo build --locked --release -j 2 -p hito-server && cp /build/target/release/hito-server /hito-server

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates ffmpeg curl && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home hito && mkdir /data && chown hito:hito /data
COPY --from=server-build /hito-server /usr/local/bin/hito-server
COPY --from=web-build /web/dist /app/web
USER hito
ENV HITO_DATABASE=/data/hito.db HITO_WEB_DIR=/app/web HITO_MUSIC_ROOT=/music
EXPOSE 8080
VOLUME ["/data"]
HEALTHCHECK --interval=15s --timeout=3s CMD curl -f http://localhost:8080/health || exit 1
ENTRYPOINT ["hito-server"]

FROM rust-tools AS desktop-tools
RUN useradd --uid 10001 --create-home hito-desktop
RUN apt-get update && apt-get install -y --no-install-recommends libwebkit2gtk-4.1-dev libasound2-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev librsvg2-bin xvfb xauth xdotool openbox dbus-x11 webkit2gtk-driver gstreamer1.0-alsa gstreamer1.0-plugins-bad pulseaudio fonts-dejavu-core python3 && rm -rf /var/lib/apt/lists/*
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/driver-target cargo install tauri-driver --version 2.1.0 --locked --target-dir /build/driver-target
COPY --from=node:22-bookworm-slim@sha256:c3de60bf2f9dd0ac6370e6117950ff62d6e339527e7472301c9c78a017978392 /usr/local/bin/node /usr/local/bin/node
ENV CARGO_TARGET_DIR=/build/desktop-target
