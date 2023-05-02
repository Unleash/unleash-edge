FROM gcr.io/distroless/cc-debian11

RUN apt install libssl-dev

COPY target/aarch64-unknown-linux-gnu/release/unleash-edge /unleash-edge
ENTRYPOINT ["/unleash-edge"]