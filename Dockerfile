FROM gcr.io/distroless/cc

COPY target/aarch64-unknown-linux-gnu/release/unleash-edge /unleash-edge
ENTRYPOINT ["/unleash-edge"]