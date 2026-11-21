# The read-only markban viewer (markban-app) as one small image.
# Build: docker build -t markban-viewer .
# Run:   see crates/markban-app/README.md, section "Hosting".

FROM rust:1-bookworm AS build
RUN rustup target add wasm32-unknown-unknown \
 && cargo install dioxus-cli --version 0.7.10 --locked
WORKDIR /src
COPY Cargo.toml Cargo.lock clippy.toml ./
COPY .cargo .cargo
COPY crates crates
COPY xtask xtask
RUN dx build -p markban-app --release --web \
 && mkdir -p /out \
 && cp -r target/dx/markban-app/release/web/. /out/

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends git ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data --create-home markban
COPY --from=build /out /app
USER markban
WORKDIR /data
ENV IP=0.0.0.0 \
    PORT=8080 \
    MARKBAN_GIT_DIR=/data/repo
EXPOSE 8080
VOLUME /data
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD ["/app/server", "--healthcheck"]
ENTRYPOINT ["/app/server"]
