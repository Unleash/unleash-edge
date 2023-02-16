import http from 'k6/http';

import { sleep } from 'k6';

export const options = {
  duration: '10s',
  vus: 50,
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<10'] // (95th percentile should be < 10 ms)
  }
};

export default function () {
  http.get('http://localhost:3063/api/client/features', { 'headers': { 'Authorization': `${__ENV.TOKEN}` } });
  sleep(0.2);
}
