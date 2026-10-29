FROM node:22-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
COPY CHANGELOG*.md /src/
RUN npm run build

FROM rust:1.89-bookworm AS rust
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY agents/ agents/
COPY CHANGELOG*.md ./
COPY --from=web /src/web/dist/ web/dist/
RUN cargo build --locked --release --package wakeit-gateway

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates util-linux && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home wakeit
COPY --from=rust /src/target/release/wakeit-gateway /usr/local/bin/wakeit-gateway
COPY deploy/gateway.yaml /app/gateway.yaml
USER 10001:10001
WORKDIR /app
EXPOSE 8080
ENTRYPOINT ["wakeit-gateway"]
CMD ["start", "--config", "/app/gateway.yaml"]
