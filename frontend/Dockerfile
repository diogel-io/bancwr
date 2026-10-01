# Build stage. Never shipped: only .output is copied out of it.
FROM node:24-slim AS build

WORKDIR /app

# pnpm at exactly the version in package.json's packageManager field, through corepack, rather than
# whatever `npm install -g pnpm` resolved on the day.
RUN corepack enable

# Copy package files
COPY package.json pnpm-lock.yaml ./

# The lockfile as committed: a build never resolves different versions from the ones CI tested.
RUN pnpm install --frozen-lockfile

# Copy source (see .dockerignore: the host's node_modules and build output stay out)
COPY . .

# Build app
RUN pnpm build

# Runtime: distroless Node 24 (#16). No shell, apt, npm, corepack, perl or gnutls, which carried
# the old image's findings; the container's only job is `node .output/server/index.mjs`. The
# :nonroot tag runs as UID 65532. The tag floats on purpose: the weekly rebuild picks up its fixes.
FROM gcr.io/distroless/nodejs24-debian13:nonroot

WORKDIR /app

# Copy built files from Nitro
COPY --from=build /app/.output ./.output

# Expose port
EXPOSE 3000

# Set environment variables
ENV HOST=0.0.0.0
ENV PORT=3000

# Release version, supplied by the release workflow from GitVersion. See docs/releasing.md.
ARG BANCWR_VERSION=0.0.0
ENV BANCWR_VERSION=${BANCWR_VERSION}

# The image's entrypoint is node, so the command is the script alone.
CMD [".output/server/index.mjs"]
