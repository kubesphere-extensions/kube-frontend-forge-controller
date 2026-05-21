# syntax=docker/dockerfile:1.7

FROM rust:1.93.1-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY template ./template
COPY xtask ./xtask

RUN cargo build --release -p frontend-forge-controller --bins

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

WORKDIR /app
COPY --from=builder /app/target/release/frontend-forge-controller /usr/local/bin/frontend-forge-controller
COPY --from=builder /app/target/release/fi-to-fe-migrator /usr/local/bin/fi-to-fe-migrator

ENTRYPOINT ["/usr/local/bin/frontend-forge-controller"]
