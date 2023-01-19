FROM alpine:3.17

WORKDIR /app
COPY target/aarch64-unknown-linux-musl/release/unleash-edge /app/
ENTRYPOINT ["/app/unleash-edge"]