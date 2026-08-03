FROM rust:1.97.1-bookworm AS builder

ARG APP
WORKDIR /workspace

# The base image already pins the compiler. Omitting rust-toolchain.toml avoids
# rustup downloading development-only components for every service image.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
COPY migrations ./migrations

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
EXPOSE 8080 9090

USER 65532:65532
ENTRYPOINT ["/usr/local/bin/agentx-service"]
