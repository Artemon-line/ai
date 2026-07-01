# syntax=docker/dockerfile:1

# ------------------------------------------------------------------------------
# Stage 1: Build
# ------------------------------------------------------------------------------

FROM rust:1.96-alpine AS builder

ENV OPENSSL_STATIC=1

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconf cmake make g++

WORKDIR /src

# ------------------------------------------------------------------------------
# Cache Build
# ------------------------------------------------------------------------------

# Cache dependency builds: copy only manifests first, then
# create stub source files so `cargo build` resolves and
# compiles all dependencies without the real source code.
#
# Build context is the monorepo root so we can reach both
# ai/ and praxis/ (the AI workspace has path deps into
# ../praxis/).

# AI workspace manifests
COPY ai/Cargo.toml ai/Cargo.lock ./ai/
COPY ai/apis/Cargo.toml ./ai/apis/Cargo.toml
COPY ai/filters/Cargo.toml ./ai/filters/Cargo.toml
COPY ai/server/Cargo.toml ./ai/server/Cargo.toml

# The server crate has a build.rs that discovers external filter
# crates via cargo metadata for build-time auto-registration.
COPY ai/server/build.rs ./ai/server/build.rs

# Praxis dependency manifests (path deps in ../praxis/)
COPY praxis/Cargo.toml praxis/Cargo.lock ./praxis/
COPY praxis/core/Cargo.toml ./praxis/core/Cargo.toml
COPY praxis/filter/Cargo.toml ./praxis/filter/Cargo.toml
COPY praxis/filter/proto/Cargo.toml ./praxis/filter/proto/Cargo.toml
COPY praxis/protocol/Cargo.toml ./praxis/protocol/Cargo.toml
COPY praxis/tls/Cargo.toml ./praxis/tls/Cargo.toml

# The proto crate has a build.rs that compiles vendored .proto files.
COPY praxis/filter/proto/build.rs ./praxis/filter/proto/build.rs
COPY praxis/filter/proto/proto ./praxis/filter/proto/proto

# Strip workspace members not needed for the binary.
RUN sed -i '/xtask/d; /tests\//d' ai/Cargo.toml
RUN sed -i '/xtask/d; /benchmarks/d; /tests\//d; /filter\/ext-proc/d; /server/d' praxis/Cargo.toml

# Create stub source files for all crates.
RUN mkdir -p ai/apis/src ai/filters/src ai/server/src \
    praxis/core/src praxis/filter/src praxis/filter/proto/src \
    praxis/protocol/src praxis/tls/src \
    && echo '//! stub' > ai/apis/src/lib.rs \
    && echo '//! stub' > ai/filters/src/lib.rs \
    && echo '//! stub' > ai/server/src/lib.rs \
    && printf '//! stub\nfn main() {}\n' > ai/server/src/main.rs \
    && echo '//! stub' > praxis/core/src/lib.rs \
    && echo '//! stub' > praxis/filter/src/lib.rs \
    && echo '//! stub' > praxis/filter/proto/src/lib.rs \
    && echo '//! stub' > praxis/protocol/src/lib.rs \
    && echo '//! stub' > praxis/tls/src/lib.rs

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/ai/target \
    cd ai && cargo build --release -p praxis-ai-proxy

# ------------------------------------------------------------------------------
# Cache Tricks
# ------------------------------------------------------------------------------

# Replace stubs with real source for both AI and praxis crates,
# then rebuild. Only the project crates recompile; all external
# dependencies are cached.
COPY ai/apis/src ./ai/apis/src
COPY ai/filters/src ./ai/filters/src
COPY ai/server/src ./ai/server/src
COPY ai/examples ./ai/examples
COPY praxis/core/src ./praxis/core/src
COPY praxis/filter/src ./praxis/filter/src
COPY praxis/filter/proto/src ./praxis/filter/proto/src
COPY praxis/protocol/src ./praxis/protocol/src
COPY praxis/tls/src ./praxis/tls/src

RUN find ai/apis/src ai/filters/src ai/server/src \
    praxis/core/src praxis/filter/src praxis/filter/proto/src \
    praxis/protocol/src praxis/tls/src \
    -name '*.rs' -exec touch {} +

# ------------------------------------------------------------------------------
# Build
# ------------------------------------------------------------------------------

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/ai/target \
    cd ai && cargo build --release -p praxis-ai-proxy \
    && cp target/release/praxis-ai /usr/local/bin/praxis-ai

# ------------------------------------------------------------------------------
# Stage 2: Runtime
# ------------------------------------------------------------------------------

FROM alpine:3.23

LABEL org.opencontainers.image.source="https://github.com/praxis-proxy/ai" \
    org.opencontainers.image.description="Praxis AI proxy server" \
    org.opencontainers.image.licenses="MIT"

RUN apk add --no-cache ca-certificates \
    && addgroup -S praxis \
    && adduser -S -G praxis -h /nonexistent -s /sbin/nologin praxis \
    && mkdir -p /etc/praxis

COPY --from=builder --chown=root:root --chmod=0555 \
    /usr/local/bin/praxis-ai /usr/local/bin/praxis-ai

USER praxis:praxis

WORKDIR /etc/praxis

EXPOSE 8080 9901

HEALTHCHECK --interval=5s --timeout=3s --start-period=2s \
    CMD wget -qO- http://127.0.0.1:9901/healthy || exit 1

ENTRYPOINT ["praxis-ai"]
