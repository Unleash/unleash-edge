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
* `-w`, `--workers <WORKERS>` — How many workers should be started to handle requests. Defaults to number of physical cpus

  Default value: `16`
* `--tls-enable` — Should we bind TLS

  Default value: `false`
* `--tls-server-key <TLS_SERVER_KEY>` — Server key to use for TLS - Needs to be a path to a file
* `--tls-server-cert <TLS_SERVER_CERT>` — Server Cert to use for TLS - Needs to be a path to a file
* `--tls-server-port <TLS_SERVER_PORT>` — Port to listen for https connection on (will use the interfaces already defined)

  Default value: `3043`
* `--instance-id <INSTANCE_ID>` — Instance id. Used for metrics reporting

  Default value: `01HJ3BRG3VA0252FZFTS2JM3QB`
* `-a`, `--app-name <APP_NAME>` — App name. Used for metrics reporting

  Default value: `unleash-edge`
* `--markdown-help`
* `--trust-proxy` — By enabling the trust proxy option. Unleash Edge will have knowledge that it's sitting behind a proxy and that the X-Forward-\* header fields may be trusted, which otherwise may be easily spoofed. Edge will use this to populate its context's  remoteAddress field If you need to only trust specific ips or CIDR, enable this flag and then set `--proxy-trusted-servers`
* `--proxy-trusted-servers <PROXY_TRUSTED_SERVERS>` — Tells Unleash Edge which servers to trust the X-Forwarded-For. Accepts explicit Ip addresses or Cidrs (127.0.0.1/16). Accepts a comma separated list or multiple instances of the flag. E.g `--proxy-trusted-servers "127.0.0.1,192.168.0.1"` and `--proxy-trusted-servers 127.0.0.1 --proxy-trusted-servers 192.168.0.1` are equivalent
* `--disable-all-endpoint` — Set this flag to true if you want to disable /api/proxy/all and /api/frontend/all Because returning all toggles regardless of their state is a potential security vulnerability, these endpoints can be disabled

  Default value: `false`
* `--edge-request-timeout <EDGE_REQUEST_TIMEOUT>` — Timeout for requests to Edge

  Default value: `5`
* `-l`, `--log-format <LOG_FORMAT>` — Which log format should Edge use

  Default value: `plain`

  Possible values: `plain`, `json`, `pretty`

* `--token-header <TOKEN_HEADER>` — token header to use for edge authorization

  Default value: `Authorization`



## `unleash-edge edge`

Run in edge mode

**Usage:** `unleash-edge edge [OPTIONS] --upstream-url <UPSTREAM_URL>`

###### **Options:**

* `-u`, `--upstream-url <UPSTREAM_URL>` — Where is your upstream URL. Remember, this is the URL to your instance, without any trailing /api suffix
* `-b`, `--backup-folder <BACKUP_FOLDER>` — A path to a local folder. Edge will write feature and token data to disk in this folder and read this back after restart. Mutually exclusive with the --redis-url option
* `-m`, `--metrics-interval-seconds <METRICS_INTERVAL_SECONDS>` — How often should we post metrics upstream?

  Default value: `60`
* `-f`, `--features-refresh-interval-seconds <FEATURES_REFRESH_INTERVAL_SECONDS>` — How long between each refresh for a token

  Default value: `10`
* `--token-revalidation-interval-seconds <TOKEN_REVALIDATION_INTERVAL_SECONDS>` — How long between each revalidation of a token

  Default value: `3600`
* `-t`, `--tokens <TOKENS>` — Get data for these client tokens at startup. Accepts comma-separated list of tokens. Hot starts your feature cache
* `-H`, `--custom-client-headers <CUSTOM_CLIENT_HEADERS>` — Expects curl header format (-H <HEADERNAME>: <HEADERVALUE>) for instance `-H X-Api-Key: mysecretapikey`
* `-s`, `--skip-ssl-verification` — If set to true, we will skip SSL verification when connecting to the upstream Unleash server

  Default value: `false`
* `--pkcs8-client-certificate-file <PKCS8_CLIENT_CERTIFICATE_FILE>` — Client certificate chain in PEM encoded X509 format with the leaf certificate first. The certificate chain should contain any intermediate certificates that should be sent to clients to allow them to build a chain to a trusted root
* `--pkcs8-client-key-file <PKCS8_CLIENT_KEY_FILE>` — Client key is a PEM encoded PKCS#8 formatted private key for the leaf certificate
* `--pkcs12-identity-file <PKCS12_IDENTITY_FILE>` — Identity file in pkcs12 format. Typically this file has a pfx extension
* `--pkcs12-passphrase <PKCS12_PASSPHRASE>` — Passphrase used to unlock the pkcs12 file
* `--upstream-certificate-file <UPSTREAM_CERTIFICATE_FILE>` — Extra certificate passed to the client for building its trust chain. Needs to be in PEM format (crt or pem extensions usually are)
* `--service-account-token <SERVICE_ACCOUNT_TOKEN>` — Service account token. Used to create client tokens if receiving a frontend token we don't have data for
* `--upstream-request-timeout <UPSTREAM_REQUEST_TIMEOUT>` — Timeout for requests to the upstream server

  Default value: `5`
* `--upstream-socket-timeout <UPSTREAM_SOCKET_TIMEOUT>` — Socket timeout for requests to upstream

  Default value: `5`
* `--redis-url <REDIS_URL>`
* `--redis-password <REDIS_PASSWORD>`
* `--redis-username <REDIS_USERNAME>`
* `--redis-port <REDIS_PORT>`
* `--redis-host <REDIS_HOST>`
* `--redis-secure`

  Default value: `false`
* `--redis-scheme <REDIS_SCHEME>`

  Default value: `redis`

  Possible values: `tcp`, `tls`, `redis`, `rediss`, `redis-unix`, `unix`

* `--token-header <TOKEN_HEADER>` — Token header to use for both edge authorization and communication with the upstream server

  Default value: `Authorization`



## `unleash-edge offline`

Run in offline mode

**Usage:** `unleash-edge offline [OPTIONS]`

###### **Options:**

* `-b`, `--bootstrap-file <BOOTSTRAP_FILE>` — The file to load our features from. This data will be loaded at startup
* `-t`, `--tokens <TOKENS>` — Tokens that should be allowed to connect to Edge. Supports a comma separated list or multiple instances of the `--tokens` argument
* `-r`, `--reload-interval <RELOAD_INTERVAL>` — The interval in seconds between reloading the bootstrap file. Disabled if unset or 0

  Default value: `0`



## `unleash-edge health`

Perform a health check against a running edge instance

**Usage:** `unleash-edge health [OPTIONS]`

###### **Options:**

* `-e`, `--edge-url <EDGE_URL>` — Where the instance you want to health check is running

  Default value: `http://localhost:3063`
* `-c`, `--ca-certificate-file <CA_CERTIFICATE_FILE>` — If you're hosting Edge using a self-signed TLS certificate use this to tell healthcheck about your CA



## `unleash-edge ready`

Perform a ready check against a running edge instance

**Usage:** `unleash-edge ready [OPTIONS]`

###### **Options:**

* `-e`, `--edge-url <EDGE_URL>` — Where the instance you want to health check is running

  Default value: `http://localhost:3063`
* `-c`, `--ca-certificate-file <CA_CERTIFICATE_FILE>` — If you're hosting Edge using a self-signed TLS certificate use this to tell the readychecker about your CA



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

