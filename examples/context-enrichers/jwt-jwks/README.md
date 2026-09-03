# JWT JWKS Context Enricher

This example runs Edge with a JavaScript context enricher that reads a bearer
token from the `X-Context-JWT` header, validates it against a JWKS endpoint, and
sets `context.userId` from a JWT claim.

The compose file also starts a small JWKS mock server. The mock server generates
an RSA key at startup, serves the public key from `/.well-known/jwks.json`, and
issues test tokens from `/token`.

This example does not start Unleash for you. Point it at either a local Unleash
instance or a hosted Unleash instance. This requires an enterprise Unleash
version with an Edge license.

## Local Unleash

Use `host.docker.internal`, not `localhost`. From inside the Edge container,
`localhost` points back at the Edge container, not at the host running Unleash.

```sh
UPSTREAM_URL=http://host.docker.internal:4242 \
TOKENS=*:development.8400d1451101f5d05c0817c03ae5286371369311121bb114f4e268f3 \
docker compose -f examples/context-enrichers/jwt-jwks/compose.yml up --build
```

The compose file maps `host.docker.internal` to Docker's host gateway for Linux.
Using the host machine's LAN IP also works.

## Hosted Unleash

```sh
UPSTREAM_URL=https://app.unleash-hosted.com/<your-instance> \
TOKENS=<your-client-api-token> \
docker compose -f examples/context-enrichers/jwt-jwks/compose.yml up --build
```

## Try It

Fetch a signed token from the mock JWKS server:

```sh
TOKEN=$(curl -s 'http://localhost:8080/token?userId=jwks-user' | jq -r .token)
```

Call Edge's frontend API with your frontend token in `Authorization` and the
mock JWT in `X-Context-JWT`:

```sh
curl \
  -H 'Authorization: <your-frontend-token>' \
  -H 'X-Context-JWT: Bearer '"$TOKEN" \
  -H 'UNLEASH-APPNAME: example' \
  'http://localhost:3063/api/frontend/all'
```

The enricher verifies the bearer token using `JWKS_URL`, validates `iss`, `aud`,
`exp`, and `nbf`, then sets `context.userId` from the claim configured by
`JWT_USER_ID_CLAIM`.

The Node worker inherits Edge's environment, so the compose file configures the
enricher with:

- `JWKS_URL=http://jwks:8080/.well-known/jwks.json`
- `JWT_ISSUER=edge-context-enricher-example`
- `JWT_AUDIENCE=unleash-edge`
- `JWT_USER_ID_CLAIM=sub`
- `JWT_HEADER=x-context-jwt`
