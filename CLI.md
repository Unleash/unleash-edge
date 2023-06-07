# Command-Line Help for `unleash-edge`

This document contains the help content for the `unleash-edge` command-line program.

**Command Overview:**

* [`unleash-edge`↴](#unleash-edge)
* [`unleash-edge edge`↴](#unleash-edge-edge)
* [`unleash-edge offline`↴](#unleash-edge-offline)
* [`unleash-edge health`↴](#unleash-edge-health)

## `unleash-edge`

**Usage:** `unleash-edge [OPTIONS] <COMMAND>`

###### **Subcommands:**

* `edge` — Run in edge mode
* `offline` — Run in offline mode
* `health` — Perform a health check against a running edge instance

###### **Options:**

* `-p`, `--port <PORT>` — Which port should this server listen for HTTP traffic on

  Default value: `3063`
* `-i`, `--interface <INTERFACE>` — Which interfaces should this server listen for HTTP traffic on

  Default value: `0.0.0.0`
* `-b`, `--base-path <BASE_PATH>` — Which base path should this server listen for HTTP traffic on

  Default value: ``
* `-w`, `--workers <WORKERS>` — How many workers should be started to handle requests. Defaults to number of physical cpus

  Default value: `8`
* `--enable-post-features` — Exposes the api/client/features endpoint for POST requests. This may be removed in a future release

  Default value: `false`
* `--tls-enable` — Should we bind TLS

  Default value: `false`
* `--tls-server-key <TLS_SERVER_KEY>` — Server key to use for TLS
* `--tls-server-cert <TLS_SERVER_CERT>` — Server Cert to use for TLS
* `--tls-server-port <TLS_SERVER_PORT>` — Port to listen for https connection on (will use the interfaces already defined)

  Default value: `3043`
* `--instance-id <INSTANCE_ID>` — Instance id. Used for metrics reporting

  Default value: `01H2AV7Z2V237GM56669ZCGSKY`
* `-a`, `--app-name <APP_NAME>` — App name. Used for metrics reporting

  Default value: `unleash-edge`
* `--markdown-help`



## `unleash-edge edge`

Run in edge mode

**Usage:** `unleash-edge edge [OPTIONS] --upstream-url <UPSTREAM_URL>`

###### **Options:**

* `-u`, `--upstream-url <UPSTREAM_URL>` — Where is your upstream URL. Remember, this is the URL to your instance, without any trailing /api suffix
* `--redis-url <REDIS_URL>`
* `--redis-password <REDIS_PASSWORD>`
* `--redis-username <REDIS_USERNAME>`
* `--redis-port <REDIS_PORT>`
* `--redis-host <REDIS_HOST>`
* `--redis-secure`

  Default value: `false`
* `--redis-scheme <REDIS_SCHEME>`

  Default value: `redis`

  Possible values: `redis`, `rediss`, `redis-unix`, `unix`

* `-b`, `--backup-folder <BACKUP_FOLDER>` — A path to a local folder. Edge will write feature and token data to disk in this folder and read this back after restart. Mutually exclusive with the --redis-url option
* `-m`, `--metrics-interval-seconds <METRICS_INTERVAL_SECONDS>` — How often should we post metrics upstream?

  Default value: `60`
* `-f`, `--features-refresh-interval-seconds <FEATURES_REFRESH_INTERVAL_SECONDS>` — How long between each refresh for a token

  Default value: `10`
* `--token-revalidation-interval-seconds <TOKEN_REVALIDATION_INTERVAL_SECONDS>` — How long between each revalidation of a token

  Default value: `3600`
* `-t`, `--tokens <TOKENS>` — Get data for these client tokens at startup. Hot starts your feature cache
* `-H`, `--custom-client-headers <CUSTOM_CLIENT_HEADERS>` — Expects curl header format (-H <HEADERNAME>: <HEADERVALUE>) for instance `-H X-Api-Key: mysecretapikey`
* `-s`, `--skip-ssl-verification` — If set to true, we will skip SSL verification when connecting to the upstream Unleash server

  Default value: `false`
* `--pkcs8-client-certificate-file <PKCS8_CLIENT_CERTIFICATE_FILE>` — Client certificate chain in PEM encoded X509 format with the leaf certificate first. The certificate chain should contain any intermediate certificates that should be sent to clients to allow them to build a chain to a trusted root
* `--pkcs8-client-key-file <PKCS8_CLIENT_KEY_FILE>` — Client key is a PEM encoded PKCS#8 formatted private key for the leaf certificate
* `--pkcs12-identity-file <PKCS12_IDENTITY_FILE>` — Identity file in pkcs12 format. Typically this file has a pfx extension
* `--pkcs12-passphrase <PKCS12_PASSPHRASE>` — Passphrase used to unlock the pkcs12 file
* `--upstream-certificate-file <UPSTREAM_CERTIFICATE_FILE>` — Extra certificate passed to the client for building its trust chain. Needs to be in PEM format (crt or pem extensions usually are)
* `--service-account-token <SERVICE_ACCOUNT_TOKEN>` — Service account token. Used to create client tokens if receiving a frontend token we don't have data for



## `unleash-edge offline`

Run in offline mode

**Usage:** `unleash-edge offline [OPTIONS]`

###### **Options:**

* `-b`, `--bootstrap-file <BOOTSTRAP_FILE>`
* `-t`, `--tokens <TOKENS>`



## `unleash-edge health`

Perform a health check against a running edge instance

**Usage:** `unleash-edge health [OPTIONS]`

###### **Options:**

* `-e`, `--edge-url <EDGE_URL>` — Where the instance you want to health check is running

  Default value: `http://localhost:3063`
* `-c`, `--ca-certificate-file <CA_CERTIFICATE_FILE>` — If you're hosting Edge using a self-signed TLS certificate use this to tell healthcheck about your CA



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

