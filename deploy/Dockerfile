FROM rust:1.88-bookworm AS builder

WORKDIR /build
ENV CARGO_TARGET_DIR=/build/crates/server/target
COPY . .
RUN cargo build --manifest-path crates/server/Cargo.toml --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home mnema \
    && mkdir -p /data/vault \
    && chown -R mnema:mnema /data

COPY --from=builder /build/crates/server/target/release/mnema-server /usr/local/bin/mnema-server

USER mnema
WORKDIR /data

ENV MNEMA_SERVER_ADDR=0.0.0.0:8080 \
    MNEMA_STORAGE_BACKEND=sqlite \
    MNEMA_VAULT_PATH=/data/vault \
    MNEMA_SQLITE_PATH=/data/mnema.sqlite \
    MNEMA_TIMEZONE=Asia/Tokyo \
    MNEMA_TIMEZONE_OFFSET=+09:00 \
    MNEMA_PLANNING_START=09:00 \
    MNEMA_PLANNING_END=17:00 \
    MNEMA_WEB_REFRESH_SECONDS=30 \
    MNEMA_AUTOMATION_MODE=off \
    MNEMA_AUTOMATION_INTERVAL_SECONDS=0 \
    RUST_LOG=mnema_server=info

EXPOSE 8080
VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:8080/api/health > /dev/null || exit 1

ENTRYPOINT ["mnema-server"]
