import http from 'k6/http';
import { check, sleep } from 'k6';
const URL = "https://sandbox.getunleash.io/moneybags/"
export const options = {
    vus: 150,
    duration: '30s',
    thresholds: {
        http_req_duration: ['p(95) < 500']
    }
};
function randomToken() {
    const chars = 'a';
    let hash = '';
    for (let i = 0; i < 64; i++) {
        hash += chars.charAt(Math.floor(Math.random() * chars.length));
    }
    return `*:development.${hash}`;
}
export default function () {
    const headers = {
        Authorization: randomToken(),
        Connection: 'close'
        // Authorization: TOKEN,
    };
    const res = http.post(`${URL}api/client/features/`, {
        headers,
        timeout: '10s',
    });
    console.log(`Response status: ${res.status}`);
    check(res, {
        'status is 403': (r) => r.status === 403,
    });
}
