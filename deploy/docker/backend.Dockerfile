FROM rust:1.97.1-bookworm AS builder

ARG APP
ARG CARGO_PACKAGE
WORKDIR /workspace

# The base image already pins the compiler. Omitting rust-toolchain.toml avoids
# rustup downloading development-only components for every service image.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
COPY tests/fixtures ./tests/fixtures
COPY xtask ./xtask
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
    fi \
    && if [ "$APP" = "platform-control" ]; then \
        cargo build --release --locked --package platform-control --bin v2-04-fixture \
        && cp target/release/v2-04-fixture /tmp/agentx-p3-fixture; \
    else \
        cp /bin/false /tmp/agentx-p3-fixture; \
    fi

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/agentx-service /usr/local/bin/agentx-service
COPY --from=builder /tmp/agentx-p3-fixture /usr/local/bin/agentx-p3-fixture

ENV AGENTX_BIND_ADDR=0.0.0.0:8080
EXPOSE 8080 3128 3129 9091 9092

USER 65532:65532
ENTRYPOINT ["/usr/local/bin/agentx-service"]
