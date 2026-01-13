# syntax=docker/dockerfile:1

FROM rust:1.92-bookworm AS builder

WORKDIR /repo

# Copy only what this crate needs (keeps build context small with a good .dockerignore).
COPY crates/bindings ./crates/bindings
COPY crates/observability ./crates/observability
COPY apps/indexer ./apps/indexer

WORKDIR /repo/apps/indexer

RUN cargo build --release --bin indexer --bin migrate

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /repo/apps/indexer/target/release/indexer /usr/local/bin/indexer
COPY --from=builder /repo/apps/indexer/target/release/migrate /usr/local/bin/migrate

ENV RUST_LOG=info

ENTRYPOINT ["/usr/local/bin/indexer"]
