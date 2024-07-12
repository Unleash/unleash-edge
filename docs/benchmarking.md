# Benchmarking

## Performance

Unleash Edge will scale linearly with CPU. There are k6 benchmarks in the benchmark folder. We've already got some
initial numbers from [hey](https://github.com/rakyll/hey).

Do note that the number of requests Edge can handle does depend on the total size of your toggle response. That is, Edge
is faster if you only have 10 toggles with 1 strategy each, than it will be with 1000 toggles with multiple strategies
on each. Benchmarks here were run with data fetched from the Unleash demo instance (roughly 100kB (350 features / 200
strategies)) as well as against a small dataset of 5 features with one strategy on each.

Edge was started using
`docker run --cpus="<cpu>" --memory=128M -p 3063:3063 -e UPSTREAM_URL=<upstream> -e TOKENS="<client token>" unleashorg/unleash-edge:edge -w <number of cpus rounded up to closest integer> edge`

Then we run hey against the proxy endpoint, evaluating toggles

### Large Dataset (350 features (100kB))

```shell
$ hey -z 10s -H "Authorization: <frontend token>" http://localhost:3063/api/frontend`
```

| CPU | Memory | RPS   | Endpoint      | p95   | Data transferred |
|-----|--------|-------|---------------|-------|------------------|
| 0.1 | 6.7 Mi | 600   | /api/frontend | 103ms | 76Mi             |
| 1   | 6.7 Mi | 6900  | /api/frontend | 7.4ms | 866Mi            |
| 4   | 9.5    | 25300 | /api/frontend | 2.4ms | 3.2Gi            |
| 8   | 15     | 40921 | /api/frontend | 1.6ms | 5.26Gi           |

and against our client features endpoint.

```shell
$ hey -z 10s -H "Authorization: <client token>" http://localhost:3063/api/client/features
```

| CPU | Memory observed | RPS   | Endpoint             | p95   | Data transferred |
|-----|-----------------|-------|----------------------|-------|------------------|
| 0.1 | 11 Mi           | 309   | /api/client/features | 199ms | 300 Mi           |
| 1   | 11 Mi           | 3236  | /api/client/features | 16ms  | 3 Gi             |
| 4   | 11 Mi           | 12815 | /api/client/features | 4.5ms | 14 Gi            |
| 8   | 17 Mi           | 23207 | /api/client/features | 2.7ms | 26 Gi            |

### Small Dataset (5 features (2kB))

```shell
$ hey -z 10s -H "Authorization: <frontend token>" http://localhost:3063/api/frontend`
```

| CPU | Memory  | RPS    | Endpoint      | p95   | Data transferred |
|-----|---------|--------|---------------|-------|------------------|
| 0.1 | 4.3 Mi  | 3673   | /api/frontend | 93ms  | 9Mi              |
| 1   | 6.7 Mi  | 39000  | /api/frontend | 1.6ms | 80Mi             |
| 4   | 6.9 Mi  | 100020 | /api/frontend | 600μs | 252Mi            |
| 8   | 12.5 Mi | 141090 | /api/frontend | 600μs | 324Mi            |

and against our client features endpoint.

```shell
$ hey -z 10s -H "Authorization: <client token>" http://localhost:3063/api/client/features
```

| CPU | Memory observed | RPS    | Endpoint             | p95   | Data transferred |
|-----|-----------------|--------|----------------------|-------|------------------|
| 0.1 | 4 Mi            | 3298   | /api/client/features | 92ms  | 64 Mi            |
| 1   | 4 Mi            | 32360  | /api/client/features | 2ms   | 527Mi            |
| 4   | 11 Mi           | 95838  | /api/client/features | 600μs | 2.13 Gi          |
| 8   | 17 Mi           | 129381 | /api/client/features | 490μs | 2.87 Gi          |