# Unleash Edge

[![Documentation](https://docs.rs/unleash-edge/badge.svg?version=latest)](https://docs.rs/unleash-edge/latest)
[![Dependency Status](https://deps.rs/crate/unleash-edge/19.11.0/status.svg)](https://deps.rs/crate/unleash-edge/19.11.0)
[![CI](https://github.com/Unleash/unleash-edge/actions/workflows/test-with-coverage.yaml/badge.svg)](https://github.com/Unleash/unleash-edge/actions/workflows/test-with-coverage.yaml)
[![Coverage Status](https://coveralls.io/repos/github/Unleash/unleash-edge/badge.svg?branch=main)](https://coveralls.io/github/Unleash/unleash-edge?branch=main)
![downloads](https://img.shields.io/crates/d/unleash-edge.svg)

## License

This repository contains both open-source and enterprise-only components.

- The enterprise crates under `crates/enterprise/` are licensed under a commercial license (see [LICENSE-ENTERPRISE.md](./LICENSE-ENTERPRISE.md)).
- The open-source crates are licensed under the MIT license (see [LICENSE](./LICENSE)).

Please refer to each crate's `Cargo.toml` for the exact license applying to that crate.
 
> [!WARNING]
> The open-source version of Unleash Edge is in long-term maintenance mode, with **end-of-life scheduled for December 31, 2026**. We recommend that customers migrate to [Enterprise Edge](https://docs.getunleash.io/unleash-edge).

## Overview

Unleash Edge is a fast and lightweight proxy layer between your Unleash API and SDKs. It acts as a read replica of your
Unleash instance and is designed to help you scale Unleash. It allows you to support thousands of connected SDKs without
increasing the number of requests you make to your Unleash instance.

If you're running the Enterprise build, see the [Enterprise Edge documentation](https://docs.getunleash.io/unleash-edge) for licensing, configuration and deployment considerations.

Edge supports both client-side and server-side SDKs and has multi-environment and project awareness. You can daisy-chain
Edge instances to support more complex setups, such as multi-cloud deployments.

Key features:

- **Performance**: Edge uses in-memory caching and can run close to your end-users. A single instance can handle tens to
  hundreds of thousands of requests per second.
- **Resilience**: Edge is designed to survive restarts and maintain functionality even if you lose connection to your
  Unleash server.
- **Security**: Edge supports frontend applications without exposing sensitive data to end-users or to Unleash.

You can run Edge in two different modes: **edge** or **offline**. To learn about the different modes and other Edge
concepts, visit [Modes of operation](https://docs.getunleash.io/unleash-edge/deploy#modes-of-operation).

Unleash Edge is the successor to Unleash Proxy. For help with migrating from Proxy to Edge, refer to
the [migration guide](https://docs.getunleash.io/unleash-edge/migrate-from-proxy).

If you're looking for the simplest way to connect your client SDKs, explore
our [Frontend API](https://docs.getunleash.io/reference/front-end-api). For additional recommendations on scaling your
feature flag system, see
our [Best practices for building and scaling feature flags](https://docs.getunleash.io/topics/feature-flags/feature-flag-best-practices)
guide.

## Quickstart

Our recommended approach is to bootstrap Edge with a client API token and upstream URL as command line arguments or
container environment variables.

To run Edge in Docker:

```shell
docker run -it -p 3063:3063 -e UPSTREAM_URL=<your_unleash_instance> -e TOKENS=<your_client_token> unleashorg/unleash-edge:<version> edge
```

For example:

```shell
docker run -it -p 3063:3063 -e UPSTREAM_URL=https://app.unleash-hosted.com/testclient -e TOKENS='*:development.4a798ad11cde8c0e637ff19f3287683ebc21d23d607c641f2dd79daa54' unleashorg/unleash-edge:<version> edge
```

## Versioning and availability

Unleash Edge is versioned and released independently of [Unleash](https://github.com/Unleash/unleash). To use Unleash
Edge, you need Unleash version 4.15 or later. We recommend using the latest versions of Unleash and Unleash Edge to
ensure optimal performance and access to the latest features and security updates.

Unleash Edge does not have full feature parity with Unleash. Some features, such as filtering feature flags by tags, are
not supported.

## Getting Unleash Edge

Unleash Edge is distributed as a binary and as a Docker image.

- **Binary**:
    - Downloadable from our [Releases page](https://github.com/Unleash/unleash-edge/releases/latest). Available for
      Linux x86_64, Windows x86_64, Darwin (OS X) x86_64, and Darwin (OS X) aarch64 (M1/M2 Macs).
    - If you have the [Rust toolchain](https://rustup.rs) installed, you can build a binary for the platform you're
      running by cloning this repository and running `cargo build --release`. The binary will be located in
      `./target/release`.
- **Docker**: The Docker image is available on:
    - Docker Hub: `unleashorg/unleash-edge:<version>`.
    - GitHub Packages: `ghcr.io/unleash/unleash-edge:<version>`.

## Running Unleash Edge

The `docker run` command supports the same [CLI arguments](/docs/CLI.md) that are available when running a binary.

To run Edge in **edge** mode, use the command `edge`. This is built from `HEAD` on each commit.

```shell
docker run -p 3063:3063 -e UPSTREAM_URL=<your_unleash_instance> -e TOKENS=<your_client_token> unleashorg/unleash-edge:<version> edge
```

To run Edge in **offline** mode, use the command `offline` and provide a volume with your feature toggles file. An
example is available inside the examples folder.

```shell
docker run -v ./examples:/edge/data -p 3063:3063 -e BOOTSTRAP_FILE=/edge/data/features.json -e CLIENT_TOKENS=<your_client_token_1,your_client_token_2> unleashorg/unleash-edge:<version> offline
```

### Connecting SDKs

Once Edge is up and running, your SDKs should connect to EDGE_URL/api. For example, `http://localhost:3063/api`.

## Additional resources

- [Edge overview and concepts](https://docs.getunleash.io/unleash-edge)
- [CLI](/docs/CLI.md)
- [Deploying Edge](https://docs.getunleash.io/unleash-edge/deploy)
- [How tokens work](https://docs.getunleash.io/unleash-edge/deploy#tokens)
- [Troubleshooting](https://docs.getunleash.io/unleash-edge/deploy#troubleshooting)
- [Migrating from Unleash Proxy](https://docs.getunleash.io/unleash-edge/migrate-from-proxy)
- [Performance benchmarking](/docs/benchmarking.md)
- [Contributors guide](/CONTRIBUTING.md) and [development guide](/docs/development-guide.md)
