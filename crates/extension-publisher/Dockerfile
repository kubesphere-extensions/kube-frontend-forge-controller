# syntax=docker/dockerfile:1.7

ARG PUBLISHER_RUNTIME_IMAGE=debian:bookworm-slim

FROM rust:1.93.1-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY xtask ./xtask

RUN cargo build --release -p frontend-forge-extension-publisher

FROM debian:bookworm-slim AS ksbuilder
ARG TARGETARCH
ARG KSBUILDER_VERSION=0.4.7

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tar \
    && rm -rf /var/lib/apt/lists/* \
    && case "${TARGETARCH}" in \
         amd64|arm64) arch="${TARGETARCH}" ;; \
         *) echo "unsupported TARGETARCH=${TARGETARCH}" >&2; exit 1 ;; \
       esac \
    && curl -fsSL "https://github.com/kubesphere/ksbuilder/releases/download/v${KSBUILDER_VERSION}/ksbuilder_${KSBUILDER_VERSION}_linux_${arch}.tar.gz" -o /tmp/ksbuilder.tar.gz \
    && tar -xzf /tmp/ksbuilder.tar.gz -C /tmp \
    && find /tmp -type f -name ksbuilder -exec cp {} /usr/local/bin/ksbuilder \; \
    && chmod +x /usr/local/bin/ksbuilder \
    && /usr/local/bin/ksbuilder version

# Keep the publisher runtime separate from controller/packager images so
# ksbuilder version changes do not affect them.
FROM ${PUBLISHER_RUNTIME_IMAGE} AS runtime

WORKDIR /work
COPY --from=ksbuilder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=ksbuilder /usr/local/bin/ksbuilder /usr/local/bin/ksbuilder
COPY --from=builder /app/target/release/frontend-forge-extension-publisher /usr/local/bin/frontend-forge-extension-publisher

ENTRYPOINT ["/usr/local/bin/frontend-forge-extension-publisher"]
