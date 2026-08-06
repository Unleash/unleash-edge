import http from 'k6/http';
import { check } from 'k6';

// Hot-path frontend evaluation benchmark.
//
// Unlike unleash-edge/benchmarks/frontendendpoint.js (a GET with no context),
// this POSTs a body carrying a rotating `userId`, so Edge's context enrichment
// actually fires and hits the enricher once per request.
//
// Env vars:
//   URL       base URL WITH trailing slash   (default http://localhost:3063/)
//   TOKEN     Authorization header value     (frontend token)
//   SUBJECTS  size of the userId key space   (default 1000000)
//   SUBJECT   pin every request to one id    (optional; for cache-hit runs)
//   VUS       virtual users                  (default 50)
//   DURATION  test duration                  (default 30s)
//   MAX_P95_MS failed-threshold ceiling      (default 50; hot path is a net hop)

const url = __ENV.URL ?? 'http://localhost:3063/';
const token = __ENV.TOKEN ?? '*:development.unleash-insecure-frontend-api-token';
const subjects = Number(__ENV.SUBJECTS ?? 1000000);
const pinned = __ENV.SUBJECT;

export const options = {
    vus: Number(__ENV.VUS ?? 50),
    duration: __ENV.DURATION ?? '30s',
    thresholds: {
        http_req_failed: ['rate<0.01'],
        http_req_duration: [`p(95)<${Number(__ENV.MAX_P95_MS ?? 50)}`],
    },
};

export default function () {
    const index = (Number(__VU) * 100000 + Number(__ITER)) % subjects;
    const userId = pinned ?? `subject-${String(index).padStart(12, '0')}`;
    const response = http.post(`${url}api/frontend`, JSON.stringify({ userId }), {
        headers: { Authorization: token, 'Content-Type': 'application/json' },
    });
    check(response, { 'status is 200': (r) => r.status === 200 });
}
