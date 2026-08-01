FROM rust:1.97-bookworm AS builder

ARG APP
WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY services ./services

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/workspace/target \
    cargo build --release --locked --bin "$APP" \
    && cp "target/release/$APP" /tmp/agentx-service

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/agentx-service /usr/local/bin/agentx-service

ENV AGENTX_BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

USER 65532:65532
ENTRYPOINT ["/usr/local/bin/agentx-service"]
