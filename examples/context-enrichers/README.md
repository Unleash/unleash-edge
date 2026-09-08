# Context Enricher Examples

These examples show different JavaScript context enrichers for Edge.

- `simple/`: reads `x-user-id` from request headers and maps it to `context.userId`.
- `semver-build/`: bundles an enricher that uses `semver` and maps an application version header to a release channel context property.
- `jwt-jwks/`: validates a JWT from a request header against a JWKS endpoint and maps a token claim to `context.userId`.
