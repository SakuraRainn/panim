# The image for the check of the build without Nix on Ubuntu 24.04 (`docker/check.sh`).
#
# It has the system packages, Node.js, pnpm, rustup with the Rust of `rust-toolchain.toml`, and the TDLib of PANIM
# from `scripts/build-tdlib.sh` in `/work/target/tdlib`. No part comes from Nix.
FROM docker.io/library/ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
# The libraries of Tauri (https://v2.tauri.app/start/prerequisites/), the tools of the TDLib build, and the tools of
# the check.
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential ca-certificates cmake curl file gperf patch pkg-config xz-utils \
      libssl-dev zlib1g-dev libsqlite3-dev \
      libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libxdo-dev libayatana-appindicator3-dev \
    && rm -rf /var/lib/apt/lists/*

# Node.js, the same major version as the dev shell. The SHA-256 comes from SHASUMS256.txt of nodejs.org.
ARG NODE_VERSION=24.22.2
ARG NODE_SHA256=48cc0e21e048790e54c0cf5749c2725005313d652651c40b7a21feb9697aaf75
ARG PNPM_VERSION=12.9.0
RUN curl -fsSL -o /tmp/node.tar.xz "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
    && echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum -c - \
    && tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 \
    && rm /tmp/node.tar.xz \
    && npm install -g "pnpm@${PNPM_VERSION}"

# rustup and the Rust of `rust-toolchain.toml`. `docker/check.sh` gives the version.
ARG RUST_VERSION
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN test -n "$RUST_VERSION" \
    && curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
         --default-toolchain "$RUST_VERSION" -c clippy -c rustfmt \
    && rustc --version

# The TDLib of PANIM. This layer stays in the cache of Docker until the script or the patch changes.
COPY scripts/build-tdlib.sh /work/scripts/build-tdlib.sh
COPY nix/tdlib-no-direct.patch /work/nix/tdlib-no-direct.patch
# `TDLIB_JOBS` sets the number of jobs. If it is empty, the script uses the cores and the free memory.
ARG TDLIB_JOBS=
RUN /work/scripts/build-tdlib.sh --work /tmp/tdlib-build ${TDLIB_JOBS:+--jobs "$TDLIB_JOBS"} && rm -rf /tmp/tdlib-build
