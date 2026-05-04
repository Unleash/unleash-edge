import http from 'k6/http';
import { check } from 'k6';
import { Counter } from 'k6/metrics';

const metricsSent = new Counter('metrics_sent');
const metricsErrors = new Counter('metrics_errors');
const featuresFetched = new Counter('features_fetched');
const featuresErrors = new Counter('features_errors');

const URL = __ENV.URL || 'http://127.0.0.1:3063/';
const TOKEN = __ENV.TOKEN;
const FEATURE_NAMES = (
    __ENV.FEATURES || 'feature-1,feature-2,feature-3,feature-4,feature-5'
).split(',');

if (!TOKEN) {
    throw new Error('Missing TOKEN env var. Run with: k6 run -e TOKEN=... script.js');
}

// Soak test: sustained moderate load over extended period to find memory leaks / degradation.
// Use constant-arrival-rate for a steady, predictable load.
const METRICS_RPS  = Number(__ENV.METRICS_RPS  || 200);
const FEATURES_RPS = Number(__ENV.FEATURES_RPS || 100);
const DURATION     = __ENV.DURATION || '30m';

export const options = {
    scenarios: {
        soak_metrics: {
            executor: 'constant-arrival-rate',
            exec: 'postMetrics',
            rate: METRICS_RPS,
            timeUnit: '1s',
            duration: DURATION,
            preAllocatedVUs: 200,
            maxVUs: 500,
        },
        soak_features: {
            executor: 'constant-arrival-rate',
            exec: 'getFeatures',
            rate: FEATURES_RPS,
            timeUnit: '1s',
            duration: DURATION,
            preAllocatedVUs: 100,
            maxVUs: 300,
        },
    },
    thresholds: {
        http_req_failed: ['rate<0.01'],
        'http_req_duration{scenario:soak_metrics}': ['p(95)<200', 'p(99)<500'],
        'http_req_duration{scenario:soak_features}': ['p(95)<50'],
        metrics_errors: ['count<50'],
        features_errors: ['count<50'],
    },
};

function buildMetricsPayload(vuId) {
    const now = new Date();
    const start = new Date(now.getTime() - 60000);

    const features = {};
    const count = Math.floor(Math.random() * FEATURE_NAMES.length) + 1;
    for (let i = 0; i < count; i++) {
        const feature = FEATURE_NAMES[i % FEATURE_NAMES.length];
        features[feature] = {
            yes: Math.floor(Math.random() * 100) + 1,
            no: Math.floor(Math.random() * 20),
            variants: {},
        };
    }

    return JSON.stringify({
        appName: `soak-test-app-${vuId % 10}`,
        instanceId: `instance-${vuId}`,
        bucket: {
            start: start.toISOString(),
            stop: now.toISOString(),
            toggles: features,
        },
        impactMetrics: [
            {
                name: `soak_counter_${vuId % 5}`,
                type: 'counter',
                help: 'A soak test counter',
                samples: [
                    {
                        value: Math.floor(Math.random() * 1000),
                        labels: { appName: `soak-test-app-${vuId % 10}` },
                    },
                ],
            },
            {
                name: `soak_histogram_${vuId % 3}`,
                type: 'histogram',
                help: 'A soak test histogram',
                samples: [
                    {
                        labels: { appName: `soak-test-app-${vuId % 10}` },
                        count: Math.floor(Math.random() * 500) + 10,
                        sum: Math.random() * 5000,
                        buckets: [
                            { le: 1.0,    count: Math.floor(Math.random() * 50) },
                            { le: 5.0,    count: Math.floor(Math.random() * 200) },
                            { le: 10.0,   count: Math.floor(Math.random() * 400) },
                            { le: '+Inf', count: Math.floor(Math.random() * 500) },
                        ],
                    },
                ],
            },
        ],
    });
}

export function postMetrics() {
    const payload = buildMetricsPayload(__VU);
    const res = http.post(`${URL}api/frontend/client/metrics`, payload, {
        headers: {
            'Content-Type': 'application/json',
            Authorization: `${TOKEN}`,
        },
    });

    const ok = check(res, {
        'metrics status is 2xx': (r) => r.status >= 200 && r.status < 300,
    });

    if (ok) {
        metricsSent.add(1);
    } else {
        metricsErrors.add(1);
    }
}

export function getFeatures() {
    const res = http.get(`${URL}api/frontend`, {
        headers: { Authorization: `${TOKEN}` },
    });

    const ok = check(res, {
        'features status is 200': (r) => r.status === 200,
    });

    if (ok) {
        featuresFetched.add(1);
    } else {
        featuresErrors.add(1);
    }
}
