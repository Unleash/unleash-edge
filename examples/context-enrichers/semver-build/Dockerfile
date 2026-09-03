FROM docker.io/rust:1.96-bookworm AS builder

WORKDIR /app

COPY . .

RUN cargo build --release --features enterprise

FROM docker.io/node:22-bookworm-slim

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/unleash-edge /app/unleash-edge

RUN mkdir -p /app/context-enrichers

USER node

EXPOSE 3063

ENTRYPOINT ["/app/unleash-edge"]
