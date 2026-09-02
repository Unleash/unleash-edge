# Context Enricher Example

This example runs Edge with a JavaScript context enricher. It does not start
Unleash for you; point it at either a local Unleash instance or a hosted
Unleash instance. This requires an enterprise Unleash version with an Edge license.

## Local Unleash

Use `host.docker.internal`, not `localhost`. From inside the Edge container,
`localhost` points back at the Edge container, not at the host running Unleash.

```sh
UPSTREAM_URL=http://host.docker.internal:4242 \
TOKENS=default:development.unleash-insecure-api-token \
docker compose -f examples/context-enrichers/compose.yml up --build
```

The compose file maps `host.docker.internal` to Docker's host gateway for Linux.
Using the host machine's LAN IP also works.

## Hosted Unleash

```sh
UPSTREAM_URL=https://app.unleash-hosted.com/<your-instance> \
TOKENS=<your-client-api-token> \
docker compose -f examples/context-enrichers/compose.yml up --build
```

## Try It

```sh
curl \
  -H 'Authorization: <your-frontend-token>' \
  -H 'x-user-id: enriched-user' \
  'http://localhost:3063/api/frontend/all'
```

The enricher in `simple-enricher.js` reads `x-user-id`, sets `context.userId`.
