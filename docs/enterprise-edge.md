# Enterprise Edge

Enterprise Edge is the commercial build of Unleash Edge. It extends the open-source runtime with additional functionality for licensed [Unleash Enterprise](https://www.getunleash.io/pricing) installations. It supports the same base commands and configuration as the OSS binary, but requires a valid Enterprise license to start and access its extended capabilities.

## Enterprise-only features

Enterprise Edge enables the following features:

- **Streaming:** Maintains a streaming connection to the upstream Edge or Unleash instance instead of polling. Enable with `--streaming` or `STREAMING=true`. When using a streaming-capable SDK, Edge can also stream updates to SDK clients. Streaming is in early access; consult your Unleash Enterprise contact for compatibility notes.

- **Edge observability:** Enterprise Edge reports heartbeat and instance state to Unleash Enterprise. This data is displayed in the Unleash Admin UI to assist with monitoring replica health, token usage, and deployment status.

Enterprise Edge is also available as a managed service for Unleash Enterprise Cloud deployments.

## Getting Enterprise Edge

### License requirements

Enterprise Edge requires a new Enterprise plan license issued to your Unleash instance. Licenses are provided through the Unleash sales team or your customer success representative. Once the license is applied to your Unleash instance, Enterprise Edge will validate itself against that license during startup.

### Download the image

The Enterprise container image can be pulled from Docker Hub:

```shell
docker pull unleashorg/unleash-edge-enterprise:<version>
```

All [CLI arguments and environment variables](/docs/CLI.md) documented for Unleash Edge apply to this image. Replace `<version>` with the version you want to run. We recommend staying up to date with the latest release. Available tags can be viewed on
[Docker Hub](https://hub.docker.com/r/unleashorg/unleash-edge-enterprise/tags).

## Persistence and cold-start reliability

Enterprise Edge always serves evaluations from in-memory state. Persistence is used only during startup to restore previously validated license, token, and feature data when the upstream Unleash instance is unavailable.

Enterprise Edge supports the following persistence options:

- **Redis** (`--redis-url`/`REDIS_URL`): Snapshots are persisted to a shared Redis cluster. Preferable when you already have Redis running as part of your infrastructure.
- **Amazon S3** (`--s3-bucket-name`/`S3_BUCKET_NAME`): Edge writes periodic snapshot files to S3 and restores them during startup. Suitable when an S3 bucket is already available or when simple durable storage is preferred over a running Redis service.

A local file-based store is also available, but it is intended for development and single-node setups. It is not recommended for production deployments or multi-replica environments.

If no persistence backend is configured, Enterprise Edge relies solely on in-memory state. In this configuration, any restart requires direct contact with the upstream Unleash instance to revalidate licenses and tokens before Edge can begin serving traffic.

If you need a persistence option not listed here, contact support.

## Quickstart

Enterprise Edge is deployed the same way as the standard Edge image. Provide the upstream URL and client tokens using the documented environment variables or CLI flags:

```shell
docker run -it \
  -p 3063:3063 \
  -e UPSTREAM_URL=<https://your-unleash-instance.com> \
  -e TOKENS=<your_client_token> \
  unleashorg/unleash-edge-enterprise:<version> edge
```
The Enterprise image exposes all the API endpoints from the OSS build, along with additional endpoints used for enterprise features, so existing SDKs can be redirected without application changes.
