# MCP over stdio. Sylphx Platform builds the web app from Dockerfile.web.
FROM rust:1.100.0-bookworm AS build
WORKDIR /source
RUN apt-get update && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock rust-toolchain.toml README.md LICENSE ./
COPY .cargo ./.cargo
COPY crates ./crates
COPY vendor ./vendor
RUN cargo build --release --locked -p anymd --bin anymd

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 anymd
COPY --from=build /source/target/release/anymd /usr/local/bin/anymd
COPY LICENSE /usr/share/licenses/anymd/LICENSE
WORKDIR /data
USER anymd
LABEL org.opencontainers.image.source="https://github.com/SylphxAI/anymd" \
      org.opencontainers.image.licenses="MIT" \
      io.modelcontextprotocol.server.name="io.github.SylphxAI/anymd"
ENTRYPOINT ["anymd", "mcp"]
