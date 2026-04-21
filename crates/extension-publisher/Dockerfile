# syntax=docker/dockerfile:1.7

FROM rust:1.93.1-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY xtask ./xtask

RUN cargo build --release -p frontend-forge-extension-publisher

# This runtime image must provide the `ksbuilder` binary. Keep it separate from
# controller/packager images so ksbuilder version changes do not affect them.
ARG PUBLISHER_RUNTIME_IMAGE=debian:bookworm-slim
FROM ${PUBLISHER_RUNTIME_IMAGE} AS runtime

WORKDIR /work
COPY --from=builder /app/target/release/frontend-forge-extension-publisher /usr/local/bin/frontend-forge-extension-publisher

ENTRYPOINT ["/usr/local/bin/frontend-forge-extension-publisher"]
