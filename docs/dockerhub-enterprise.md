# Unleash Enterprise Edge

Unleash Enterprise Edge is a high-performance proxy layer between Unleash and your SDKs. It runs close to your
applications, keeps feature flag data cached, and helps large installations scale client and server SDK traffic without
increasing load on the Unleash server.

This image contains the enterprise build of Unleash Edge. It requires a valid Enterprise Edge license and is intended for
customers using the enterprise-only Edge capabilities documented at
[docs.getunleash.io/unleash-edge](https://docs.getunleash.io/unleash-edge).

## Quickstart

Run Enterprise Edge in edge mode with an upstream Unleash URL and one or more client API tokens:

```shell
docker run -p 3063:3063 \
  -e UPSTREAM_URL=<your_unleash_instance> \
  -e TOKENS=<your_client_token> \
  unleashorg/unleash-edge-enterprise:<version> edge
```

Once Edge is running, point SDKs at:

```text
http://localhost:3063/api
```

Replace `localhost:3063` with the host and port where your Edge instance is reachable.

## Image tags

Use a pinned version tag for production deployments:

```shell
docker pull unleashorg/unleash-edge-enterprise:<version>
```

The same image is also published to:

- GitHub Container Registry: `ghcr.io/unleash/unleash-edge-enterprise:<version>`
- Amazon ECR Public: `public.ecr.aws/unleashorg/unleash-edge-enterprise:<version>`

## Documentation

- [Enterprise Edge documentation](https://docs.getunleash.io/unleash-edge)
- [Deploying Edge](https://docs.getunleash.io/unleash-edge/deploy)
- [How tokens work](https://docs.getunleash.io/unleash-edge/deploy#tokens)
- [Troubleshooting](https://docs.getunleash.io/unleash-edge/deploy#troubleshooting)
- [Unleash Edge repository](https://github.com/Unleash/unleash-edge)
