# Context Enricher Examples

Enable a context enricher by passing a CommonJS module to Edge:

```sh
cargo run -p unleash-edge -- edge \
  --context-enricher-script examples/context-enrichers/01-sync-user-id.js
```

The module must export a function that accepts an Unleash context object and returns the enriched context. It may return either the context directly or a promise.

## 1. Synchronous User ID Override

`01-sync-user-id.js` is deliberately dumb: it always replaces `userId` with `"7"`.

```sh
--context-enricher-script examples/context-enrichers/01-sync-user-id.js
```

## 2. Async Delay

`02-async-delay.js` waits for 150 ms, then adds `properties.delayed = "true"`.

```sh
--context-enricher-script examples/context-enrichers/02-async-delay.js
```

## 3. Local URL Lookup

`03-local-url.js` posts the context to a hardcoded local URL:

```text
http://127.0.0.1:3210/context
```

Start the example local service in another terminal:

```sh
node examples/context-enrichers/local-url-service.js
```

Then run Edge with:

```sh
--context-enricher-script examples/context-enrichers/03-local-url.js
```

The local service returns additional context properties, and the script merges them into the original context.

Context property values must be strings because Unleash contexts deserialize `properties` as `HashMap<String, String>`.
