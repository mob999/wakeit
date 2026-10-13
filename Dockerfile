FROM node:22-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.89-bookworm AS rust
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY agents/ agents/
COPY --from=web /src/web/dist/ web/dist/
RUN cargo build --locked --release --bin wakeit

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates util-linux && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home wakeit
COPY --from=rust /src/target/release/wakeit /usr/local/bin/wakeit
COPY deploy/gateway.yaml /app/gateway.yaml
USER 10001:10001
WORKDIR /app
EXPOSE 8080
ENTRYPOINT ["wakeit"]
CMD ["gateway", "start", "--config", "/app/gateway.yaml"]
