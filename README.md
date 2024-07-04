# Unleash Edge

[![crates.io](https://img.shields.io/crates/v/unleash-edge?label=latest)](https://crates.io/crates/unleash-edge)
[![Documentation](https://docs.rs/unleash-edge/badge.svg?version=latest)](https://docs.rs/unleash-edge/latest)
![MIT licensed](https://img.shields.io/crates/l/unleash-edge.svg)
[![Dependency Status](https://deps.rs/crate/unleash-edge/19.2.0/status.svg)](https://deps.rs/crate/unleash-edge/19.2.0)
[![CI](https://github.com/Unleash/unleash-edge/actions/workflows/test-with-coverage.yaml/badge.svg)](https://github.com/Unleash/unleash-edge/actions/workflows/test-with-coverage.yaml)
[![Coverage Status](https://coveralls.io/repos/github/Unleash/unleash-edge/badge.svg?branch=main)](https://coveralls.io/github/Unleash/unleash-edge?branch=main)
![downloads](https://img.shields.io/crates/d/unleash-edge.svg)

> Warning: Unleash Edge requires Unleash v4.15.0 or higher

Unleash Edge is the successor to the [Unleash Proxy](https://docs.getunleash.io/how-to/how-to-run-the-unleash-proxy).

Unleash Edge sits between the Unleash API and your SDKs and provides a cached read-replica of your Unleash instance.
This means you can scale up your Unleash instance to thousands of connected SDKs without increasing the number of
requests you make to your Unleash instance.

Unleash Edge offers two important features:

- **Performance**: Unleash Edge caches in memory and can run close to your end-users. A single instance can handle tens
  to hundreds of thousands of requests per second.
- **Resilience**: Unleash Edge is designed to survive restarts and operate properly even if you lose connection to your
  Unleash server.

Unleash Edge is built to help you scale Unleash.

* If you're looking for the easiest way to connect your client SDKs you can check out
  our [Frontend API](https://docs.getunleash.io/reference/front-end-api).
* If you're looking to learn how to scale your own feature flag system why not check out our recommendations for
  building and scaling [feature flags](https://docs.getunleash.io/topics/feature-flags/feature-flag-best-practices)

## Migrating to Edge from the Proxy

For more info on migrating, check out the [migration guide](./migration-guide.md) that details the differences between
Edge and the Proxy and how to achieve similar behavior in Edge.

## Quickstart

Running Edge in Docker with our recommended setup:

```shell
docker run -it -p 3063:3063 -e STRICT=true -e UPSTREAM_URL=<yourunleashinstance> unleashorg/unleash-edge:<mostrecentversion> edge
```

## Deploying

See our page on [Deploying Edge](./docs/deploying.md)

## Getting Unleash Edge

Unleash Edge is distributed as a binary and as a docker image.

### Binary

- The binary is downloadable from our [Releases page](https://github.com/Unleash/unleash-edge/releases/latest).
- We're currently building for linux x86_64, windows x86_64, darwin (OS X) x86_64 and darwin (OS X) aarch64 (M1/M2 macs)

### Docker

- The docker image gets uploaded to dockerhub and Github Package registry.
- For dockerhub use the coordinates `unleashorg/unleash-edge:<version>`.
- For Github package registry use the coordinates `ghpr.io/unleash/unleash-edge:<version>`
- If you'd like to live on the edge (sic) you can use the tag `edge`. This is built from `HEAD` on each commit
- When running the docker image, the same CLI arguments that's available when running the binary is available to
  your `docker run` command. To start successfully you will need to decide which mode you're running in.
    - If running in `edge` mode your command should be
        - `docker run -p 3063:3063 -e UPSTREAM_URL=<YOUR_UNLEASH_INSTANCE> unleashorg/unleash-edge:<version> edge`
    - If running in `offline` mode you will need to provide a volume containing your feature toggles file. An example is
      available inside the examples folder. To use this, you can use the command
        - `docker run -v ./examples:/edge/data -p 3063:3063 -e BOOTSTRAP_FILE=/edge/data/features.json -e TOKENS='my-secret-123,another-secret-789' unleashorg/unleash-edge:<version> offline`

### Cargo/Rust

If you have the [Rust toolchain](https://rustup.rs) installed you can build a binary for the platform you're running by
cloning this repo and running `cargo build --release`. This will give you an `unleash-edge` binary in `./target/release`

## Concepts

See our page on [Edge concepts](./docs/concepts.md)

## Metrics

**❗ Note:** For Edge to correctly register SDK usage metrics, your Unleash instance must be v5.9.0 or newer.
**! Note:** If you're daisy chaining you will need at least Edge 17.0.0 upstream of any Edge 19.0.0 to preserve metrics.

Since Edge is designed to avoid overloading its upstream, Edge gathers and accumulates usage metrics from SDKs for a set
interval (METRICS_INTERVAL_SECONDS) before posting a batch upstream.
This reduces load on Unleash instances down to a single call every interval, instead of every single client posting to
Unleash for updating metrics.
Unleash instances running on versions older than 4.22 are not able to handle the batch format posted by Edge, which
means you won't see any metrics from clients connected to an Edge instance until you're able to update to 4.22 or newer.

## Compatibility

Unleash Edge adheres to Semantic Versioning (SemVer) on the API and CLI layers. If you're using Unleash Edge as a
library in your projects, be cautious, internal codebase changes, which might occur in any version release (including
minor and patch versions), could potentially break your implementation.

## Performance

See our page on [Edge benchmarking] [Benchmarking](./docs/benchmarking.md)

## Debugging

You can adjust the `RUST_LOG` environment variable to see more verbose log output. For example, in order to get logs
originating directly from Edge but not its dependencies, you can raise the default log level from `error` to `warning`
and set Edge to `debug`, like this:

```sh
RUST_LOG="warn,unleash_edge=debug" ./unleash-edge #<command>
```

See more about available logging and log levels at https://docs.rs/env_logger/latest/env_logger/#enabling-logging

## Troubleshooting

### Missing metrics in upstream server

#### Possible reasons

- Old Edge version. In order to guarantee metrics on newer Unleash versions, you will need to be using Edge v17.0.0 or
  newer
- Old SDK clients. We've seen some clients, particularly early Python (1.x branch) as well as earlier .NET SDKs (we
  recommend you use 4.1.5 or newer) struggle to post metrics with the strict headers Edge requires.

## Development

See our [Contributors guide](./CONTRIBUTING.md) as well as our [development-guide](./development-guide.md)
