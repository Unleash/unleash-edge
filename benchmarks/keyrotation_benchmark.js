import http from 'k6/http';
import {check, sleep} from 'k6';

export const options = {
    vus: 150,
    duration: '5s',
    thresholds: {
        http_req_duration: ['p(95) < 500']
    }
};

function randomToken() {
    const chars = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ';
    let hash = '';
    for (let i = 0; i < 64; i++) {
        hash += chars.charAt(Math.floor(Math.random() * chars.length));
    }
    return `*:development.${hash}`;
}

export default function () {
    const headers = {
        Authorization: randomToken(),
        // Authorization: TOKEN,
    };
    const res = http.post(`${__ENV.URL}api/client/metrics`, {
        headers,
        timeout: '10s',
    });
    console.log(`Response status: ${res.status}`);
    check(res, {
        'status is 403': (r) => r.status === 403,
    });
}
