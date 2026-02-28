FROM ubuntu:24.04 AS builder

ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:${PATH}

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY bridges ./bridges

# Build the CLI without desktop GUI (headless) and all bridge binaries
RUN cargo build --release -p localgpt --no-default-features \
    && cargo build --release -p localgpt-bridge-telegram \
    && cargo build --release -p localgpt-bridge-discord \
    && cargo build --release -p localgpt-bridge-cli

FROM ubuntu:24.04 AS runtime

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates bash git ripgrep \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system --gid 10001 localgpt \
    && useradd --system --uid 10001 --gid localgpt --create-home --home-dir /home/localgpt localgpt \
    && mkdir -p /home/localgpt/.localgpt \
           /home/localgpt/.cache/localgpt \
           /home/localgpt/.local/state/localgpt/locks \
           /home/localgpt/.local/share/localgpt/bridges \
    && chown -R localgpt:localgpt /home/localgpt

COPY --from=builder /app/target/release/localgpt /usr/local/bin/localgpt
COPY --from=builder /app/target/release/localgpt-bridge-telegram /usr/local/bin/localgpt-bridge-telegram
COPY --from=builder /app/target/release/localgpt-bridge-discord /usr/local/bin/localgpt-bridge-discord
COPY --from=builder /app/target/release/localgpt-bridge-cli /usr/local/bin/localgpt-bridge-cli

USER localgpt:localgpt
WORKDIR /home/localgpt

ENTRYPOINT ["localgpt"]
