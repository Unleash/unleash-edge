# Command-Line Help for `unleash-edge`

This document contains the help content for the `unleash-edge` command-line program.

**Command Overview:**

* [`unleash-edge`↴](#unleash-edge)
* [`unleash-edge edge`↴](#unleash-edge-edge)
* [`unleash-edge offline`↴](#unleash-edge-offline)
* [`unleash-edge health`↴](#unleash-edge-health)
* [`unleash-edge ready`↴](#unleash-edge-ready)

## `unleash-edge`

**Usage:** `unleash-edge [OPTIONS] <COMMAND>`

###### **Subcommands:**

* `edge` — Run in edge mode
* `offline` — Run in offline mode
* `health` — Perform a health check against a running edge instance
* `ready` — Perform a ready check against a running edge instance

###### **Options:**

* `-p`, `--port <PORT>` — Which port should this server listen for HTTP traffic on

  Default value: `3063`
* `-i`, `--interface <INTERFACE>` — Which interfaces should this server listen for HTTP traffic on

  Default value: `0.0.0.0`
* `-b`, `--base-path <BASE_PATH>` — Which base path should this server listen for HTTP traffic on

  Default value: ``
* `-w`, `--workers <WORKERS>` — How many workers should be started to handle requests. Defaults to number of physical
  cpus

  Default value: `<physical_cpus>`
* `--tls-enable` — Should we bind TLS

  Default value: `false`
* `--tls-server-key <TLS_SERVER_KEY>` — Server key to use for TLS - Needs to be a path to a file
* `--tls-server-cert <TLS_SERVER_CERT>` — Server Cert to use for TLS - Needs to be a path to a file
* `--tls-server-port <TLS_SERVER_PORT>` — Port to listen for https connection on (will use the interfaces already
  defined)

  Default value: `3043`
* `--cors-origin <CORS_ORIGIN>` — Sets
  the [Access-Control-Allow-Origin](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin)
  header to this value
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
* `--allow-list <ALLOW_LIST>` — Configures the AllowList middleware to only accept requests from IPs that belong to the
  CIDRs configured here. Defaults to 0.0.0.0/0, ::/0 (ALL Ips v4 and v6)
* `--deny-list <DENY_LIST>` — Configures the DenyList middleware to deny requests from IPs that belong to the CIDRs
  configured here. Defaults to denying no IPs
* `--instance-id <INSTANCE_ID>` — Instance id. Used for metrics reporting

  Default value: `unleash-edge@<random ulid>`
* `-a`, `--app-name <APP_NAME>` — App name. Used for metrics reporting

  Default value: `unleash-edge`
* `--trust-proxy` — By enabling the trust proxy option. Unleash Edge will have knowledge that it's sitting behind a
  proxy and that the X-Forward-\* header fields may be trusted, which otherwise may be easily spoofed. Edge will use
  this to populate its context's remoteAddress field If you need to only trust specific ips or CIDR, enable this flag
  and then set `--proxy-trusted-servers`
* `--proxy-trusted-servers <PROXY_TRUSTED_SERVERS>` — Tells Unleash Edge which servers to trust the X-Forwarded-For.
  Accepts explicit Ip addresses or Cidrs (127.0.0.1/16). Accepts a comma separated list or multiple instances of the
  flag. E.g `--proxy-trusted-servers "127.0.0.1,192.168.0.1"` and
  `--proxy-trusted-servers 127.0.0.1 --proxy-trusted-servers 192.168.0.1` are equivalent
* `--disable-all-endpoint` — Set this flag to true if you want to disable /api/proxy/all and /api/frontend/all Because
  returning all toggles regardless of their state is a potential security vulnerability, these endpoints can be disabled

  Default value: `false`
* `--edge-request-timeout <EDGE_REQUEST_TIMEOUT>` — Timeout for requests to Edge

  Default value: `5`
* `--edge-keepalive-timeout <EDGE_KEEPALIVE_TIMEOUT>` — Keepalive timeout for requests to Edge

  Default value: `5`
* `-l`, `--log-format <LOG_FORMAT>` — Which log format should Edge use

  Default value: `plain`

  Possible values: `plain`, `json`, `pretty`

* `--edge-auth-header <EDGE_AUTH_HEADER>` — Header to use for edge authorization
* `--upstream-auth-header <UPSTREAM_AUTH_HEADER>` — Header to use for upstream authorization
* `--token-header <TOKEN_HEADER>` — token header to use for edge authorization
* `--disable-metrics-batch-endpoint` — Disables /internal-backstage/metricsbatch endpoint

  This endpoint shows the current cached client metrics
* `--disable-metrics-endpoint` — Disables /internal-backstage/metrics endpoint

  Typically used for prometheus scraping metrics.
* `--disable-features-endpoint` — Disables /internal-backstage/features endpoint

  Used to show current cached features across environments
* `--disable-tokens-endpoint` — Disables /internal-backstage/tokens endpoint

  Used to show tokens used to refresh feature caches, but also tokens already validated/invalidated against upstream
* `--disable-instance-data-endpoint` — Disables /internal-backstage/instancedata endpoint

  Used to show instance data for the edge instance.

## `unleash-edge edge`

Run in edge mode

**Usage:** `unleash-edge edge [OPTIONS] --upstream-url <UPSTREAM_URL> [PEM_CERT_FILE]`

###### **Arguments:**

* `<PEM_CERT_FILE>`

###### **Options:**

* `-u`, `--upstream-url <UPSTREAM_URL>` — Where is your upstream URL. Remember, this is the URL to your instance,
  without any trailing /api suffix
* `-b`, `--backup-folder <BACKUP_FOLDER>` — A path to a local folder. Edge will write feature and token data to disk in
  this folder and read this back after restart. Mutually exclusive with the --redis-url option
* `-m`, `--metrics-interval-seconds <METRICS_INTERVAL_SECONDS>` — How often should we post metrics upstream?

  Default value: `60`
* `-f`, `--features-refresh-interval-seconds <FEATURES_REFRESH_INTERVAL_SECONDS>` — How long between each refresh for a
  token

  Default value: `15`
* `--token-revalidation-interval-seconds <TOKEN_REVALIDATION_INTERVAL_SECONDS>` — How long between each revalidation of
  a token

  Default value: `3600`
* `-t`, `--tokens <TOKENS>` — Get data for these client tokens at startup. Accepts comma-separated list of tokens. Hot
  starts your feature cache
* `-p`, `--pretrusted-tokens <PRETRUSTED_TOKENS>` — Set a list of frontend tokens that Edge will always trust. These
  need to either match the Unleash token format, or they're an arbitrary string followed by an @ and then an
  environment, e.g. secret-123@development
* `-H`, `--custom-client-headers <CUSTOM_CLIENT_HEADERS>` — Expects curl header format (`-H <HEADERNAME>: <HEADERVALUE>`)
  for instance `-H X-Api-Key: mysecretapikey`
* `-s`, `--skip-ssl-verification` — If set to true, we will skip SSL verification when connecting to the upstream
  Unleash server

  Default value: `false`
* `--pkcs8-client-certificate-file <PKCS8_CLIENT_CERTIFICATE_FILE>` — Client certificate chain in PEM encoded X509
  format with the leaf certificate first. The certificate chain should contain any intermediate certificates that should
  be sent to clients to allow them to build a chain to a trusted root
* `--pkcs8-client-key-file <PKCS8_CLIENT_KEY_FILE>` — Client key is a PEM encoded PKCS#8 formatted private key for the
  leaf certificate
* `--pkcs12-identity-file <PKCS12_IDENTITY_FILE>` — Identity file in pkcs12 format. Typically this file has a pfx
  extension
* `--pkcs12-passphrase <PKCS12_PASSPHRASE>` — Passphrase used to unlock the pkcs12 file
* `--upstream-certificate-file <UPSTREAM_CERTIFICATE_FILE>` — Extra certificate passed to the client for building its
  trust chain. Needs to be in PEM format (crt or pem extensions usually are)
* `--upstream-request-timeout <UPSTREAM_REQUEST_TIMEOUT>` — Timeout for requests to the upstream server

  Default value: `5`
* `--upstream-socket-timeout <UPSTREAM_SOCKET_TIMEOUT>` — Socket timeout for requests to upstream

  Default value: `5`
* `--redis-url <REDIS_URL>`
* `--redis-mode <REDIS_MODE>`

  Default value: `single`

  Possible values: `single`, `cluster`

* `--redis-password <REDIS_PASSWORD>`
* `--redis-username <REDIS_USERNAME>`
* `--redis-port <REDIS_PORT>`
* `--redis-host <REDIS_HOST>`
* `--redis-secure`

  Default value: `false`
* `--redis-scheme <REDIS_SCHEME>`

  Default value: `redis`

  Possible values: `tcp`, `tls`, `redis`, `rediss`, `redis-unix`, `unix`

* `--redis-read-connection-timeout-milliseconds <REDIS_READ_CONNECTION_TIMEOUT_MILLISECONDS>` — Timeout (in
  milliseconds) for waiting for a successful connection to redis, when restoring

  Default value: `2000`
* `--redis-write-connection-timeout-milliseconds <REDIS_WRITE_CONNECTION_TIMEOUT_MILLISECONDS>` — Timeout (in
  milliseconds) for waiting for a successful connection to redis when persisting

  Default value: `2000`
* `--s3-bucket-name <S3_BUCKET_NAME>` — Bucket name to use for storing feature and token data
* `--strict` — If set to true, Edge starts with strict behavior. Strict behavior means that Edge will refuse tokens
  outside the scope of the startup tokens

  Default value: `false`
* `--dynamic` — If set to true, Edge starts with dynamic behavior. Dynamic behavior means that Edge will accept tokens
  outside the scope of the startup tokens

  Default value: `false`
* `--prometheus-remote-write-url <PROMETHEUS_REMOTE_WRITE_URL>` — Sets a remote write url for prometheus metrics, if
  this is set, prometheus metrics will be written upstream
* `--prometheus-push-interval <PROMETHEUS_PUSH_INTERVAL>` — Sets the interval for prometheus push metrics, only relevant
  if `prometheus_remote_write_url` is set. Defaults to 60 seconds

  Default value: `60`
* `--prometheus-username <PROMETHEUS_USERNAME>`
* `--prometheus-password <PROMETHEUS_PASSWORD>`
* `--prometheus-user-id <PROMETHEUS_USER_ID>`

## `unleash-edge offline`

Run in offline mode

**Usage:** `unleash-edge offline [OPTIONS]`

###### **Options:**

* `-b`, `--bootstrap-file <BOOTSTRAP_FILE>` — The file to load our features from. This data will be loaded at startup
* `-t`, `--tokens <TOKENS>` — Tokens that should be allowed to connect to Edge. Supports a comma separated list or
  multiple instances of the `--tokens` argument (v19.4.0) deprecated "Please use --client-tokens | CLIENT_TOKENS
  instead"
* `-c`, `--client-tokens <CLIENT_TOKENS>` — Client tokens that should be allowed to connect to Edge. Supports a comma
  separated list or multiple instances of the `--client-tokens` argument
* `-f`, `--frontend-tokens <FRONTEND_TOKENS>` — Frontend tokens that should be allowed to connect to Edge. Supports a
  comma separated list or multiple instances of the `--frontend-tokens` argument
* `-r`, `--reload-interval <RELOAD_INTERVAL>` — The interval in seconds between reloading the bootstrap file. Disabled
  if unset or 0

  Default value: `0`

## `unleash-edge health`

Perform a health check against a running edge instance

**Usage:** `unleash-edge health [OPTIONS]`

###### **Options:**

* `-e`, `--edge-url <EDGE_URL>` — Where the instance you want to health check is running

  Default value: `http://localhost:3063`
* `-c`, `--ca-certificate-file <CA_CERTIFICATE_FILE>` — If you're hosting Edge using a self-signed TLS certificate use
  this to tell healthcheck about your CA

## `unleash-edge ready`

Perform a ready check against a running edge instance

**Usage:** `unleash-edge ready [OPTIONS]`

###### **Options:**

* `-e`, `--edge-url <EDGE_URL>` — Where the instance you want to health check is running

  Default value: `http://localhost:3063`
* `-c`, `--ca-certificate-file <CA_CERTIFICATE_FILE>` — If you're hosting Edge using a self-signed TLS certificate use
  this to tell the readychecker about your CA

<hr/>

<small><i>
This document was generated automatically by
<a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
