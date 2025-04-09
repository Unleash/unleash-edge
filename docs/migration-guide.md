# Migrating from the Unleash Proxy to Edge

Edge is built to be a near drop-in replacement for the Unleash proxy, but there are some differences. This guide takes
you through

- the differences between Edge and the Unleash proxy
- how to migrate from the Unleash proxy to Edge
- in cases where the feature set isn't equivalent, how to achieve the same results in Edge.

A full [Docker compose file](../examples/docker-compose.yml) is also provided. It will spin up Edge, Unleash, and Redis,
to allow you to understand the configuration options in context.

After starting the compose, you should be able to access the Unleash UI at `http://localhost:4242`, add a toggle, and
cURL the edge instance with:

``` sh
curl --location --request GET 'http://0.0.0.0:3063/api/client/features' \
--header 'Content-Type: application/json' \
--header 'Authorization: default:development.unleash-insecure-api-token' \
--data-raw ''
```

## Not supported

- [Custom Strategies](https://docs.getunleash.io/reference/custom-activation-strategies) are not supported in Edge
  today. If you need to use a custom strategy, you will need to use the Unleash proxy. We believe that the overwhelming
  majority of custom strategies are better expressed as a set
  of [strategy constraints](https://docs.getunleash.io/reference/strategy-constraints). If you have a situation where
  constraints **cannot** replace your strategy, please raise this as an issue with details on what you're trying to
  achieve. We are looking into supporting custom strategies in Edge in the future.

- Legacy proxy Tokens. If you're using the proxy, you may be using the legacy proxy token format. These are not
  supported in Edge. You will need to create a new front end SDK token in the Unleash UI and use that. These are the
  same tokens that the front end API requires. Because of the way Edge handles API tokens, this is not a feature we're
  planning to support.

- Context enrichers. The Unleash proxy provides an experimental option to automatically enrich requests with additional
  context. This is not supported in currently in Edge, we're open to adding this feature to Edge, opening an issue would
  help us to know that this is a valuable feature to you.

- JS integration. Edge is written in Rust. The Unleash proxy is written JavaScript. If you're wrapping the proxy code in
  your own JavaScript, you'll need to port that equivalent code to Rust. Generally, if you have a deep integration on
  the existing proxy, we would recommend either not moving to Edge, or to reach out to us to discuss your use case.

## Existing features in new ways

This section is to detail features that have changed in meaningful ways from the proxy implementation. These features
all exist in Edge but you may find yourself thinking about them in new ways.

- New modes of operation. Edge supports two modes of operation - Edge mode and Offline mode. Offline mode is described
  in more detail later. If you're using the proxy today, you very likely want Edge mode. Edge mode will allow Unleash to
  sync its feature information from upstream Unleash instances and serve requests from that information in a similar way
  to the proxy. Unlike the proxy you need to specify the mode on startup. Example run command:

    ``` sh
    ./edge edge --upstream-url http://localhost:4242
    ```

- API Tokens. If you're using the proxy, you'll be familiar with the idea of specifying a token for the proxy to
  communicate with Unleash. This is not strictly necessary when using Edge; Edge is smart enough to reuse the tokens
  that are sent from your backend SDKs and intelligently and dynamically determine what tokens are necessary to provide
  a full set of details to all the SDKs that it's seen. This is all handled behind the scenes for you, so if you're only
  using backend SDKs, not specifying a token is fine. In practice, this means that the very first request to Edge that
  Edge doesn't currently have data for, will result in Edge blocking that request until it can resolve that data. If you
  want to hot start your cache and avoid this problem or you're using front end SDKs with Edge, you can specify a token
  in the Edge configuration. Note that if you're using a front end SDK then you'll either need to specify a backend SDK
  token at startup or ensure that at least one request has been made to Edge with a valid SDK token that is able to
  resolve all the environments and projects that you want to use front end SDKs with.


- Backups. By default the Unleash proxy will dump all of its feature information to disk to ensure that restarts have
  all the data you need to start serving requests as fast as possible. Edge does this by default but it also allows you
  the option to set other methods of storage rather than just flat files. Today we only support Redis (if you want
  support for a storage technology that we don't support, please feel free to raise ticket or open a PR!). To use Redis,
  you'll need to specify a valid connection to your storage provider in the Edge configuration. This means that Edge
  will lazily sync its data to the specified provider in the background and read that back on startup, if present. If
  you're using Redis, you'll need to ensure that the Redis instance is available before starting Edge. Example run
  command:

    ``` sh
    ./edge edge --upstream-url http://localhost:4242 --redis-url redis://localhost:6379

    ```

- Multiple environments. The Unleash proxy only supports a single environment, provided by your API key when starting
  up. Edge does not suffer from this limitation, any connecting SDK can freely use API tokens to scope its request to an
  environment, project or both, so long as the upstream Unleash instance has that token. This is dynamically determined
  by Edge, so adding a new token to the upstream Unleash and using that token in an SDK talking to Edge will work
  without restarting Edge or changing configuration.

- Scaling out Edge. Edge is built with high performance scaling as a first class citizen. Typically, the proxy would be
  scaled out by spinning up multiple instances, due to the limitations in JS engines that prevent using all available
  cores. The other reason for potentially scaling out the proxy is that the proxy doesn't support multiple environments.
  Edge doesn't have these requirements, multiple environments are supported out the box and Edge, by default, will use
  all the processing power that's available to it to scale up. Edge runs cold. Unless you're running Edge at extreme
  scaling levels, it's very likely that you won't need to scale out your Edge instance at all; in fact, its more likely
  that you want to _reduce_ the resources available to Edge.

## Existing features with new configuration options

This section unpacks the small changes in Edge from the proxy. These are ports of existing features or configuration
that have small changes. These shouldn't affect how you use Edge and the ideas here are similar to the proxy, only small
details have changed.

- Unleash URL. The proxy requires that you specify an Unleash URL to the upstream server, in the format https:
  //{unleashUrl}/api. Edge has changed this, the URL that Edge requires is https://{unleashUrl}, without the `/api`
  suffix.

- Backend SDK support. The proxy does support connecting to backend SDKs, but it requires some configuration and setting
  some experimental feature flags. Edge supports this out the box, no additional configuration is required.

## New features

- Offline mode. Edge can be used in a purely offline mode. While the proxy does support this, it requires a little work
  to get there. Edge, on the other hand, supports this directly and out the box. This can be useful for testing or
  development environments where you don't want to have to run an Unleash instance or want to freeze your configuration
  in place. Example run command:

    ``` sh
    ./edge offline --tokens "*.development.some-test-token"  --bootstrap-file ./examples/features.json
    ```

  This will start Edge in offline mode and set the initial feature set to the contents of the `features.json` file. Edge
  will not send metrics to upstream Unleash instances, update its feature information, or dynamically resolve tokens.
  Note that you must set a token or tokens on startup in this mode - Edge will only use this set of tokens to validate
  incoming requests, this doesn't have to be a valid Unleash token, so this is very similar to the original proxy
  tokens.

- Daisy chaining. Edge is capable of both mimicking an Unleash server and talking to an Unleash server. This means that
  Edge can use another Edge instance as another upstream source. This can be extremely useful for high scaling scenarios
  where you can have a single instance close to your SDKs that retrieves all toggles for all projects and environments
  and specific Edge instances syncing to the parent Edge instance, with much more narrowly defined API tokens for
  security reasons. No extra configuration is needed to achieve this, simply point the downstream Edge instance at the
  upstream Edge instance and it will automatically resolve the tokens and features from the upstream Edge instance.
  Metrics will propagate up the daisy chain from downstream Edge instances to upstream instances until the metrics hit
  the end of the chain. There are two important details you need to be aware of when using daisy chaining with a top
  level instance in offline mode. Firstly, you won't receive any metrics for your connected SDKs - the metrics will
  propagate all the way to the final offline instance in the chain and then be discarded. Secondly, the top level
  instance needs to be started with all the API tokens necessary to serve downstream SDK requests.

## Why choose Unleash Edge over the Unleash Proxy?

Edge offers a superset of the same feature set as the Unleash Proxy and we've made sure it offers the same security and
privacy features.

However, there are a few notable differences between the Unleash Proxy and Unleash Edge:

- Unleash Edge is built to be light and fast, it handles an order of magnitude more requests per second than the Unleash
  Proxy can, while using two orders of magnitude less memory.
- All your Unleash environments can be handled by a single instance, no more running multiple instances of the Unleash
  Proxy to handle both your development and production environments.
- Backend SDKs can connect to Unleash Edge without turning on experimental feature flags.
- Unleash Edge is smart enough to dynamically resolve the tokens you use to connect to it against the upstream Unleash
  instance. This means you don't have to worry about knowing in advance what tokens your SDKs use - if you want to swap
  out the Unleash token your SDK uses, this can be done without ever restarting or worrying about Unleash Edge. Unleash
  Edge will only collect and cache data for the environments and projects you use.
