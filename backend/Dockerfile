# Multi-stage Dockerfile for Bancwr Diogel (Bunker)
# Uses cargo-chef for optimal dependency caching

# Stage 1: Chef - prepares the build recipe
FROM rust:1.94-slim-bookworm AS chef
WORKDIR /usr/src/bunker

# Install dependencies needed for cargo-chef
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    curl \
    xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef from its pinned, checksum-verified release binary rather than compiling it,
# which cost ~2 min per cold build natively and ~25 min under emulation (#39).
ARG CARGO_CHEF_VERSION=0.1.78
ARG CARGO_CHEF_SHA256_X86_64=aca691abfbfbbe00d482e0ed2249eec3091b65e96a0ce92947fb5b254d48b16d
ARG CARGO_CHEF_SHA256_AARCH64=cd59b90fce5fa84c4189648fec33404ec5491791eebdb812df5e7fca8336408d
RUN set -eu; \
    arch="$(uname -m)"; \
    case "$arch" in \
      x86_64) sha="$CARGO_CHEF_SHA256_X86_64" ;; \
      aarch64) sha="$CARGO_CHEF_SHA256_AARCH64" ;; \
      *) echo "unsupported architecture: $arch" >&2; exit 1 ;; \
    esac; \
    name="cargo-chef-${arch}-unknown-linux-musl"; \
    curl -fsSL -o /tmp/cargo-chef.tar.xz \
      "https://github.com/LukeMathWalker/cargo-chef/releases/download/v${CARGO_CHEF_VERSION}/${name}.tar.xz"; \
    echo "${sha}  /tmp/cargo-chef.tar.xz" | sha256sum -c -; \
    tar -xJf /tmp/cargo-chef.tar.xz -C /tmp; \
    install -m 0755 "/tmp/${name}/cargo-chef" /usr/local/cargo/bin/cargo-chef; \
    rm -rf /tmp/cargo-chef.tar.xz "/tmp/${name}"; \
    cargo chef --version

# Stage 2: Planner - generates the recipe.json
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder - compiles dependencies and the app
FROM chef AS builder

# Build dependencies. No OpenSSL: the release binary links only libc, libm and libgcc (#16);
# OpenSSL enters only through reqwest, a dev-dependency the release build does not compile.
RUN apt-get update && apt-get install -y \
    g++ \
    && rm -rf /var/lib/apt/lists/*

# Copy the recipe and build dependencies (this layer is cached!)
COPY --from=planner /usr/src/bunker/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source and build the app
COPY . .
RUN cargo build --release

# Stage 4: Runtime
# Distroless (#16): glibc and libgcc, which is all the binary links, and nothing else: no shell,
# apt, perl, curl, zlib or gnutls, which carried every critical finding in the old image. The
# :nonroot tag runs as UID 65532; the data volume must be writable by it (see the README).
# The tag floats on purpose: the weekly rebuild picks up its fixes.
FROM gcr.io/distroless/cc-debian13:nonroot

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

# There is no curl: the binary probes itself (`bunker healthcheck`, GET /health on BUNKER_PORT).
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD ["bunker", "healthcheck"]

ENTRYPOINT ["bunker"]
