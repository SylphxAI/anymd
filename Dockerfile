FROM rust:1.100.0-bookworm AS wasm
WORKDIR /source
RUN apt-get update && apt-get install -y --no-install-recommends python3 && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY vendor ./vendor
COPY scripts/build-wasm.sh ./scripts/build-wasm.sh
RUN bash scripts/build-wasm.sh

FROM oven/bun:1.4.4-debian AS bun
FROM node:24-bookworm-slim AS build
COPY --from=bun /usr/local/bin/bun /usr/local/bin/bun
WORKDIR /source
RUN apt-get update && apt-get install -y --no-install-recommends python3 && rm -rf /var/lib/apt/lists/*
COPY package.json bun.lock ./
COPY apps/web/package.json ./apps/web/package.json
RUN bun install --frozen-lockfile
COPY docs ./docs
COPY config/money-catalogue.json ./config/money-catalogue.json
COPY crates/anymd/src/money_docs.toml ./crates/anymd/src/money_docs.toml
COPY scripts/render-web-money.py ./scripts/render-web-money.py
COPY apps/web ./apps/web
COPY --from=wasm /source/apps/web/public/wasm ./apps/web/public/wasm
RUN bun run web:build

FROM node:24-bookworm-slim AS web
WORKDIR /app
ENV NODE_ENV=production PORT=3000 HOST=0.0.0.0
COPY --from=build --chown=node:node /source/apps/web/.output ./.output
EXPOSE 3000
USER node
CMD ["node", ".output/server/index.mjs"]
