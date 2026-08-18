FROM rust:1.97.1-bookworm AS builder

ARG APP
ARG CARGO_PACKAGE
WORKDIR /workspace

# The base image already pins the compiler. Omitting rust-toolchain.toml avoids
# rustup downloading development-only components for every service image.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
COPY migrations ./migrations
COPY openapi ./openapi

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/workspace/target \
    find crates services -type f -name '*.rs' -exec touch {} + \
    && if [ -n "$CARGO_PACKAGE" ]; then \
        CARGO_TARGET_DIR="/workspace/target/$CARGO_PACKAGE" \
            cargo build --release --locked --package "$CARGO_PACKAGE" --bin "$APP" \
        && cp "/workspace/target/$CARGO_PACKAGE/release/$APP" /tmp/agentx-service; \
    else \
        cargo build --release --locked --bin "$APP" \
        && cp "target/release/$APP" /tmp/agentx-service; \
    fi

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/agentx-service /usr/local/bin/agentx-service

ENV AGENTX_BIND_ADDR=0.0.0.0:8080
EXPOSE 8080 9092

USER 65532:65532
ENTRYPOINT ["/usr/local/bin/agentx-service"]
