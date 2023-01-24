# Unleash Edge

Unleash Edge is the successor to the Unleash Proxy.

## Running Unleash Edge

Unleash edge is compiled to a single binary. We use Clap to parse arguments/environment variables to configure Unleash Edge. All modes [See Concepts/Modes](#modes) share http configuration variables.

```shell
$ ./unleash-edge --help
Options:
  -p, --port <PORT>
          Which port should this server listen for HTTP traffic on [env: PORT=] [default: 3063]
  -i, --interface <INTERFACE>
          Which interfaces should this server listen for HTTP traffic on [env: INTERFACE=] [default: 0.0.0.0]
      --tls-enable
          Should we bind TLS [env: TLS_ENABLE=]
      --tls-server-key <TLS_SERVER_KEY>
          Server key to use for TLS [env: TLS_SERVER_KEY=]
      --tls-server-cert <TLS_SERVER_CERT>
          Server Cert to use for TLS [env: TLS_SERVER_CERT=]
      --tls-server-port <TLS_SERVER_PORT>
          Port to listen for https connection on (will use the interfaces already defined) [env: TLS_SERVER_PORT=] [default: 3043]
```

## Concepts

### Modes

We support running in various modes, from a [local version](#offline) to a full blown [edge mode](#edge) supporting dynamic keys, metrics.

#### Offline

You have a need to have full control of both the data your clients will get and which keys can be used to access the server. This mode needs a downloaded JSON dump of a result from a query against an Unleash server on the [/api/client/features](https://docs.getunleash.io/reference/api/unleash/get-client-feature) endpoint as well as a comma-separated list of keys that should be allowed to access the server.

If your keys follow the Unleash API key format `[project]:[environment].<somesecret>`, Edge will filter the features dump to match the project contained in the key. 

If you'd rather use a simple key like `secret-123`, any query against `/api/client/features` will receive the dump passed in on the command line. 

Any query against `/api/frontend` or `/api/proxy` with a valid key will receive only enabled  toggles.
To launch in this mode, run

```bash
$ ./target/debug/unleash-edge offline --help
Usage: unleash-edge offline [OPTIONS]

Options:
  -b, --bootstrap-file <BOOTSTRAP_FILE>  [env: BOOTSTRAP_FILE=]
  -c, --client-keys <CLIENT_KEYS>        [env: CLIENT_KEYS=]

```
#### Proxy
TODO: Document proxy mode


#### Edge
TODO: Document edge mode

## Development
See our [Contributors guide](./CONTRIBUTING.md) as well as our [development-guide](./development-guide.md)
