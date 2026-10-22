# Semver Build Context Enricher

This example shows how to use an npm library in a context enricher by bundling
source code into a single CommonJS artifact and using that artifact as the
enricher script.

The enricher imports `semver`, reads the `X-Application-Version` request
header, and sets `context.properties.releaseChannel` to:

- `experimental` for prerelease versions such as `1.2.3-beta.1`
- `stable` for normal versions such as `1.2.3`

The enrichment logic is intentionally small. The point of the example is the
build setup.

## Build

Install dependencies and build the bundled enricher:

```sh
cd examples/context-enrichers/semver-build
npm install
npm run build
```

This writes `dist/enricher.cjs`. The compose file mounts that built artifact
into the Edge container and points `CONTEXT_ENRICHER_SCRIPT` at it.

## Run

This example does not start Unleash for you. Point it at either a local Unleash
instance or a hosted Unleash instance. This requires an enterprise Unleash
version with an Edge license.

For local Unleash, use `host.docker.internal`, not `localhost`. From inside the
Edge container, `localhost` points back at the Edge container, not at the host
running Unleash.

```sh
UPSTREAM_URL=http://host.docker.internal:4242 \
TOKENS=*:development.8400d1451101f5d05c0817c03ae5286371369311121bb114f4e268f3 \
docker compose -f examples/context-enrichers/semver-build/compose.yml up --build
```

For hosted Unleash:

```sh
UPSTREAM_URL=https://app.unleash-hosted.com/<your-instance> \
TOKENS=<your-client-api-token> \
docker compose -f examples/context-enrichers/semver-build/compose.yml up --build
```

## Try It

To validate this against Unleash, create a feature toggle with a constraint that
requires the custom context field `releaseChannel` to equal `experimental`.

Then call Edge's frontend API with a prerelease version:

```sh
curl \
  -H 'Authorization: <your-frontend-token>' \
  -H 'X-Application-Version: 1.2.3-beta.1' \
  -H 'UNLEASH-APPNAME: example' \
  'http://localhost:3063/api/frontend/all'
```

The same request with `X-Application-Version: 1.2.3` sets
`releaseChannel=stable`.
