# Deploying

## Security considerations

Anything under /internal-backstage should be protected from public access as it exposes internal state of unleash-edge.

### CORS

`--cors-origin <CORS_ORIGIN>` — Sets
  the [Access-Control-Allow-Origin](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin)
  header to this value

To enable CORS headers, you can use the `--cors-origin` flag when starting Edge. For example:

```shell
unleash-edge edge --cors-origin "https://example.com"
```

For multiple domains, use a comma separated list or pass multiple instances of the `--cors-origin` parameter

```shell
unleash-edge edge --cors-origin "https://example.com,https://example2.com"
```
is equivalent to
```shell
unleash-edge edge --cors-origin "https://example.com" --cors-origin "https://other.example.com"
```

#### Other CORS headers

* `--cors-allowed-headers <CORS_ALLOWED_HEADERS>` — Sets
  the [Access-Control-Allow-Headers](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Headers)
  header to this value
* `--cors-max-age <CORS_MAX_AGE>` — Sets
  the [Access-Control-Max-Age](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Max-Age) header
  to this value

  Default value: `172800`
* `--cors-exposed-headers <CORS_EXPOSED_HEADERS>` — Sets
  the [Access-Control-Expose-Headers](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Expose-Headers)
  header to this value
* `--cors-methods <CORS_METHODS>` — Sets
  the [Access-Control-Allow-Methods](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Methods)
  header to this value

## Running Unleash Edge

Edge provides a range of powerful ways in which you can run it. For a standard production configuration we recommend the
following:

- Run in Edge mode: Edge mode connects to your upstream Unleash and syncs feature flags and tokens. This should
  be the default mode that you choose for most Edge configurations in production.
- Start Edge with initialization tokens: Edge mode allows you to specify a set of tokens on startup that Edge will use
  to hydrate data ahead of time. This means that Edge will have the data it requires to respond to frontend API requests
  and client API requests will not need to hydrate data on demand. **If you are running Edge behind a load balancer and
  making use of the frontend API, setting startup tokens is necessary for predictable responses from Edge.**
- Choose a appropriate scope for your initialization tokens: We recommend using one wildcard token per environment. This
  gives more predictability over the resources that Edge will use at runtime. If Edge needs to run in a sensitive
  context, starting Edge with tokens that are scoped to all the projects that downstream SDKs are expected to consume is
  okay.

Unleash Edge is compiled to a single binary. You can configure it by passing in arguments or setting environment
variables.

```shell
Usage: unleash-edge [OPTIONS] <COMMAND>

Commands:
  edge     Run in edge mode
  offline  Run in offline mode
  help     Print this message or the help of the given subcommand(s)

Options:
  -p, --port <PORT>
          Which port should this server listen for HTTP traffic on [env: PORT=] [default: 3063]
  -i, --interface <INTERFACE>
          Which interfaces should this server listen for HTTP traffic on [env: INTERFACE=] [default: 0.0.0.0]
  -b, --base-path <BASE_PATH>
          Which base path should this server listen for HTTP traffic on [env: BASE_PATH=] [default: ]
  -w, --workers <WORKERS>
          How many workers should be started to handle requests. Defaults to number of physical cpus [env: WORKERS=] [default: number of physical cpus]
      --tls-enable
          Should we bind TLS [env: TLS_ENABLE=]
      --tls-server-key <TLS_SERVER_KEY>
          Server key to use for TLS [env: TLS_SERVER_KEY=] (Needs to be a path to a file)
      --tls-server-cert <TLS_SERVER_CERT>
          Server Cert to use for TLS [env: TLS_SERVER_CERT=] (Needs to be a path to a file)
      --tls-server-port <TLS_SERVER_PORT>
          Port to listen for https connection on (will use the interfaces already defined) [env: TLS_SERVER_PORT=] [default: 3043]
      --instance-id <INSTANCE_ID>
          Instance id. Used for metrics reporting [env: INSTANCE_ID=] [default: Ulid::new()]
  -a, --app-name <APP_NAME>
          App name. Used for metrics reporting [env: APP_NAME=] [default: unleash-edge]
  -h, --help
          Print help
  -l, --log-format <LOG_FORMAT>
        Which log format should Edge use
      [env: LOG_FORMAT=]
        [default: `plain`]
      Possible values: `plain`, `json`, `pretty`
  --token-header <TOKEN_HEADER>
      token header to use for edge authorization [env: TOKEN_HEADER=] [default: Authorization]
```

### Built-in Health check

There is now (from 5.1.0) a subcommand named `health` which will ping your health endpoint and exit with status 0
provided the health endpoint returns 200 OK.

Example:

```shell
./unleash-edge health
```

will check an Edge process running on http://localhost:3063. If you're using base-path or the port variable you should
use the `-e --edge-url` CLI arg (or the EDGE_URL environment variable) to tell the health checker where edge is running.

If you're hosting Edge with a self-signed certificate using the tls cli arguments, you should use
the `--ca-certificate-file <file_containing_your_ca_and_key_in_pem_format>` flag (or the CA_CERTIFICATE_FILE environment
variable) to allow the health checker to trust the self signed certificate.

### Built-in Ready check

There is now (from 12.0.0) a subcommand named `ready` which will ping your ready endpoint and exit with status 0
provided the ready endpoint returns 200 OK and `{ status: "READY" }`. Otherwise it will return status 1 and an error
message to signal that Edge is not ready (it has not spoken to upstream or recovered from a persisted backup).

Examples:

* Edge not running:

```shell
$ ./unleash-edge ready
Error: Failed to connect to ready endpoint at http://localhost:3063/internal-backstage/ready. Failed with status None
$ echo $?
1
```

* Edge running but not populated its feature cache yet (not spoken to upstream or restored from backup)

```shell
$ ./unleash-edge ready
Error: Ready check returned a different status than READY. It returned EdgeStatus { status: NotReady }
$ echo $?
1
```

* Edge running and synchronized. I.e. READY

```shell
$ ./unleash-edge ready
OK
$ echo $?
0
```

If you're hosting Edge with a self-signed certificate using the tls cli arguments, you should use
the `--ca-certificate-file <file_containing_your_ca_and_key_in_pem_format>` flag (or the CA_CERTIFICATE_FILE environment
variable) to allow the health checker to trust the self signed certificate.


## Security considerations

Edge by default exposes quite a few debug endpoints to help you understand what is going on.

These endpoints can be sensitive, so you should protect them from public access. We recommend using a reverse proxy to protect these endpoints.
When using a reverse proxy, all endpoints under `/internal-backstage` should be protected from public access.
If you're exposing Edge to the public internet without a reverse proxy, each endpoint under `/internal-backstage` can be disabled at startup by setting
the following flags:

| CLI Flag | Environment variable | Description | URL |
| --- | --- | --- | --- |
| --disable-metrics-batch-endpoint | DISABLE_METRICS_BATCH_ENDPOINT | Disables the metrics batch endpoint | /internal-backstage/metricsbatch |
| --disable-metrics-endpoint | DISABLE_METRICS_ENDPOINT | Disables the metrics endpoint | /internal-backstage/metrics |
| --disable-tokens-endpoint | DISABLE_TOKENS_ENDPOINT | Disables the tokens endpoint | /internal-backstage/tokens |
| --disable-features-endpoint | DISABLE_FEATURES_ENDPOINT | Disables the features endpoint | /internal-backstage/features |

In addition /internal-backstage has the `/ready` and `/health` endpoints, but since these only return a 200 OK and a status, they are safe to expose.
