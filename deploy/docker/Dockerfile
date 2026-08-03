FROM rust:1.97-bookworm AS builder

WORKDIR /src
COPY . .
RUN cargo build --locked --release -p jc-memory-server

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 jc-memory \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin jc-memory \
    && mkdir -p /etc/jc-memory/retrieval /etc/jc-memory/security /var/lib/jc-memory \
    && chown -R 10001:10001 /var/lib/jc-memory

COPY --from=builder /src/target/release/jc-memory-server /usr/local/bin/jc-memory-server
COPY --from=builder /src/config/retrieval/aliases.zh-CN.yaml /etc/jc-memory/retrieval/aliases.zh-CN.yaml
COPY --from=builder /src/config/security/quarantine.zh-CN.yaml /etc/jc-memory/security/quarantine.zh-CN.yaml
COPY --from=builder /src/deploy/docker/config/server.yaml.example /etc/jc-memory/server.yaml

ENV JC_MEMORY_CONFIG_FILE=/etc/jc-memory/server.yaml

USER 10001:10001
EXPOSE 8787 8788
VOLUME ["/var/lib/jc-memory"]

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:8787/healthz >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/jc-memory-server"]
