import { htmlReport } from "https://raw.githubusercontent.com/benc-uk/k6-reporter/main/dist/bundle.js";
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

export function handleSummary(data) {
  return {
    "summary.html": htmlReport(data),
  };
}

export default function () {
  http.get('http://127.0.0.1:3063/api/proxy', { 'headers': { 'Authorization': `${__ENV.TOKEN}` } });
}
