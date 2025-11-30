FROM debian:stable-slim

RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates curl build-essential pkg-config qemu-system-x86 && \
    rm -rf /var/lib/apt/lists/*

# Install rustup + pinned nightly
ARG RUST_TOOLCHAIN=nightly-2024-06-01
ENV CARGO_HOME=/opt/cargo \
    RUSTUP_HOME=/opt/rustup
RUN mkdir -p $CARGO_HOME $RUSTUP_HOME && \
    curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain ${RUST_TOOLCHAIN} && \
    . "$CARGO_HOME/env" && \
    rustup component add rust-src llvm-tools-preview --toolchain ${RUST_TOOLCHAIN} && \
    rustup target add x86_64-unknown-uefi --toolchain ${RUST_TOOLCHAIN}

ENV PATH=$CARGO_HOME/bin:$PATH
WORKDIR /workspace

CMD ["bash"]
