FROM scratch

COPY target/aarch64-unknown-linux-musl/release/unleash-edge /unleash-edge
ENTRYPOINT ["/unleash-edge"]