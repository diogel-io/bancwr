# Multi-stage Dockerfile for Bancwr Diogel (Bunker)
# Uses cargo-chef for optimal dependency caching

# Stage 1: Chef - prepares the build recipe
FROM rust:1.94-slim-bookworm AS chef
WORKDIR /usr/src/bunker

# Install dependencies needed for cargo-chef
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-chef

# Stage 2: Planner - generates the recipe.json
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder - compiles dependencies and the app
FROM chef AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    g++ \
    && rm -rf /var/lib/apt/lists/*

# Copy the recipe and build dependencies (this layer is cached!)
COPY --from=planner /usr/src/bunker/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source and build the app
COPY . .
RUN cargo build --release

# Stage 4: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary
COPY --from=builder /usr/src/bunker/target/release/bunker /usr/local/bin/bunker

# Release version, supplied by the release workflow from GitVersion. See docs/releasing.md.
ARG BANCWR_VERSION=0.0.0
ENV BANCWR_VERSION=${BANCWR_VERSION}

# Default port configuration
ARG DEFAULT_PORT=3000
ENV BUNKER_PORT=${DEFAULT_PORT}

EXPOSE ${DEFAULT_PORT}

# Healthcheck
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:${BUNKER_PORT}/health || exit 1

CMD ["bunker"]
