# Unleash Edge

[![crates.io](https://img.shields.io/crates/v/unleash-edge?label=latest)](https://crates.io/crates/unleash-edge)
[![Documentation](https://docs.rs/unleash-edge/badge.svg?version=latest)](https://docs.rs/unleash-edge/latest)
![MIT licensed](https://img.shields.io/crates/l/unleash-edge.svg)
[![Dependency Status](https://deps.rs/crate/unleash-edge/19.9.1/status.svg)](https://deps.rs/crate/unleash-edge/19.9.1)
[![CI](https://github.com/Unleash/unleash-edge/actions/workflows/test-with-coverage.yaml/badge.svg)](https://github.com/Unleash/unleash-edge/actions/workflows/test-with-coverage.yaml)
[![Coverage Status](https://coveralls.io/repos/github/Unleash/unleash-edge/badge.svg?branch=main)](https://coveralls.io/github/Unleash/unleash-edge?branch=main)
![downloads](https://img.shields.io/crates/d/unleash-edge.svg)

## Overview

Unleash Edge is a fast and lightweight proxy layer between your Unleash API and SDKs. It acts as a read replica of your Unleash instance and is designed to help you scale Unleash. It allows you to support thousands of connected SDKs without increasing the number of requests you make to your Unleash instance.

Edge supports both client-side and server-side SDKs and has multi-environment and project awareness. You can daisy-chain Edge instances to support more complex setups, such as multi-cloud deployments.

Key features:

- **Performance**: Edge uses in-memory caching and can run close to your end-users. A single instance can handle tens to hundreds of thousands of requests per second.
- **Resilience**: Edge is designed to survive restarts and maintain functionality even if you lose connection to your Unleash server.
- **Security**: Edge supports frontend applications without exposing sensitive data to end-users or to Unleash.

You can run Edge in two different modes: **edge** or **offline**. To learn about the different modes and other Edge concepts, visit [Concepts](/docs/concepts.md).

Unleash Edge is the successor to Unleash Proxy. For help with migrating from Proxy to Edge, refer to the [migration guide](/docs/migration-guide.md).

If you're looking for the simplest way to connect your client SDKs, explore our [Frontend API](https://docs.getunleash.io/reference/front-end-api). For additional recommendations on scaling your feature flag system, see our [Best practices for building and scaling feature flags](https://docs.getunleash.io/topics/feature-flags/feature-flag-best-practices) guide.

## Quickstart

Our recommended approach is to bootstrap Edge with a client API token and upstream URL as command line arguments or container environment variables.

To run Edge in Docker:

```shell
docker run -it -p 3063:3063 -e STRICT=true -e UPSTREAM_URL=<your_unleash_instance> -e TOKENS=<your_client_token> unleashorg/unleash-edge:<version> edge
```

For example:

```shell
docker run -it -p 3063:3063 -e STRICT=true -e UPSTREAM_URL=https://app.unleash-hosted.com/testclient -e TOKENS='*:development.4a798ad11cde8c0e637ff19f3287683ebc21d23d607c641f2dd79daa54' unleashorg/unleash-edge:v19.6.2 edge
```

## Versioning and availability

Unleash Edge is versioned and released independently of [Unleash](https://github.com/Unleash/unleash). To use Unleash Edge, you need Unleash version 4.15 or later. We recommend using the latest versions of Unleash and Unleash Edge to ensure optimal performance and access to the latest features and security updates.

Unleash Edge does not have full feature parity with Unleash. Some features, such as filtering feature flags by tags, are not supported.

## Edge behaviors

> Availability: Unleash Edge v19.2+

Unleash Edge supports two behaviors when running in edge mode: **strict** and **dynamic**. We recommend using **strict** behavior, as **dynamic** is a legacy behavior. Behaviors are mutually exclusive.

### Strict behavior

When using strict behavior, Edge requires tokens at startup and refuses requests from SDKs that have a wider or different access scope than the initial tokens. Incoming requests must have a token that exactly matches the environment and projects specified in the initial tokens.

For example, if you start Edge with a wildcard token with access to the development environment (`*:development.<some_token_string>`) and your clients use various tokens with access to specific projects in the development environment, Edge filters features to only grant access to the narrower scope.

To run Edge in strict mode, use the `--strict` CLI argument or `STRICT` environment variable.

### Dynamic behavior

> Legacy behavior, use strict instead.

When using dynamic behavior, Edge validates any new client tokens against upstream. If valid, it configures a refresh job with the minimum set of tokens needed to fetch all observed projects and environments.

To run Edge in dynamic mode, use the `--dynamic` CLI argument or `DYNAMIC` environment variable.

## Getting Unleash Edge

Unleash Edge is distributed as a binary and as a Docker image.

- **Binary**:
  - Downloadable from our [Releases page](https://github.com/Unleash/unleash-edge/releases/latest). Available for Linux x86_64, Windows x86_64, Darwin (OS X) x86_64, and Darwin (OS X) aarch64 (M1/M2 Macs).
  - If you have the [Rust toolchain](https://rustup.rs) installed, you can build a binary for the platform you're running by cloning this repository and running `cargo build --release`. The binary will be located in `./target/release`.
- **Docker**: The Docker image is available on:
  -  Docker Hub: `unleashorg/unleash-edge:<version>`.
  -  GitHub Packages: `ghcr.io/unleash/unleash-edge:<version>`.

## Running Unleash Edge

The `docker run` command supports the same [CLI arguments](/docs/CLI.md) that are available when running a binary.

To run Edge in **edge** mode, use the command `edge`. This is built from `HEAD` on each commit.

```shell
docker run -p 3063:3063 -e STRICT=true -e UPSTREAM_URL=<your_unleash_instance> -e TOKENS=<your_client_token> unleashorg/unleash-edge:<version> edge
```

To run Edge in **offline** mode, use the command `offline` and provide a volume with your feature toggles file. An example is available inside the examples folder.

```shell
docker run -v ./examples:/edge/data -p 3063:3063 -e BOOTSTRAP_FILE=/edge/data/features.json -e TOKENS=<your_client_token_1,your_client_token_2> unleashorg/unleash-edge:<version> offline
```

### Client and frontend tokens in offline mode

> Availability: Unleash Edge v19.4+

Offline mode supports multiple [token types](https://docs.getunleash.io/reference/api-tokens-and-client-keys).

For [client tokens](https://docs.getunleash.io/reference/api-tokens-and-client-keys#client-tokens), use:
- `CLIENT_TOKENS` or `TOKENS` environment variables
- `--client-tokens` or `--tokens` CLI flags

For [frontend tokens](https://docs.getunleash.io/reference/api-tokens-and-client-keys#front-end-tokens), use:
- `FRONTEND_TOKENS` environment variable
- `--frontend-tokens` CLI flag

When configured this way, Edge in offline mode can validate tokens and tell daisy-chained Edges instances the type of token calling the validate endpoint.

## Metrics

> Availability: Unleash v5.9+. For daisy-chaining, ensure Edge v17+ is upstream of any Edge v19+ to preserve metrics.

Edge is designed to minimize load on its upstream by batching SDK usage metrics. Metrics are gathered over a set interval (`METRICS_INTERVAL_SECONDS`) and sent upstream in a single batch.

Unleash versions older than 4.22 cannot process these metrics, so an update is required to see metrics from clients connected to Edge.

### Prometheus integration

To monitor the health and performance of your Edge instances, you can consume Prometheus metrics at:

`http://<your-edge-url>/internal-backstage/metrics`

## Compatibility

Unleash Edge adheres to Semantic Versioning (SemVer) on the API and CLI layers. If you're using Unleash Edge as a library in your projects, note that internal changes could affect your implementation, even in minor or patch versions.

## Debugging

You can view the internal state of Edge at:

- `http://<your-edge-url>/internal-backstage/tokens`: Displays the tokens known to Edge.
- `http://<your-edge-url>/internal-backstage/features`: Shows the current state of features.

Note: The `/internal-backstage/*` endpoints should not be publicly accessible.

To enable verbose logging, adjust the `RUST_LOG` environment variable. For example, to see logs originating directly from Edge but not its dependencies, you can raise the default log level from `error` to `warning` and set Edge to `debug`, like this:

```sh
RUST_LOG="warn,unleash_edge=debug" ./unleash-edge #<command>
```

See more about available logging and log levels [here](https://docs.rs/env_logger/latest/env_logger/#enabling-logging).

## Additional resources

### Edge concepts

To learn more about Unleash Edge, see the [Concepts](/docs/concepts.md) documentation.

### CLI

For a list of available command-line arguments, see [CLI](/docs/CLI.md).

### Deploying Edge

For deployment instructions, see our [Deploying Edge](/docs/deploying.md) guide.

### Migrating from Unleash Proxy

To migrate from the Unleash Proxy to Unleash Edge, refer to the [migration guide](/docs/migration-guide.md).

### Performance benchmarking

For performance benchmarking, see our [Benchmarking](/docs/benchmarking.md) page.

### Contribution and development guide

See our [Contributors guide](/CONTRIBUTING.md) as well as our [development-guide](/docs/development-guide.md).
