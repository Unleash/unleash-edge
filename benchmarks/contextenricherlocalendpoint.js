import http from 'k6/http';

import { check } from 'k6';

export const options = {
  duration: '10s',
  vus: 50,
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<10'] // (95th percentile should be < 10 ms)
  }
};

const baseUrl = __ENV.CONTEXT_ENRICHER_URL || 'http://127.0.0.1:3210';
const url = baseUrl.endsWith('/context') ? baseUrl : `${baseUrl.replace(/\/$/, '')}/context`;

export default function () {
  const response = http.post(
    url,
    JSON.stringify({
      userId: '7',
      properties: {}
    }),
    {
      headers: {
        'Content-Type': 'application/json'
      }
    }
  );

  check(response, {
    'local enricher returned 200': (res) => res.status === 200
  });
}
