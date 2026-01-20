import http from 'k6/http';

import {sleep} from 'k6';

export const options = {
    duration: '20s',
    vus: 10,
    thresholds: {
        http_req_failed: ['rate<0.01'],
        http_req_duration: ['p(95)<10'] // (95th percentile should be < 10 ms)
    }
};

export default function () {
    http.get(`${__ENV.URL}api/frontend?userId=42`, {'headers': {'Authorization': `${__ENV.TOKEN}`}});
}
