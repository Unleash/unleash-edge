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

// Stress test: step up metrics Requests Per Second to find the breaking point,
// while a steady stream of feature reads runs concurrently.
const RPS_WARMUP   = Number(__ENV.RPS_WARMUP   || 50);
const RPS_NORMAL   = Number(__ENV.RPS_NORMAL   || 200);
const RPS_HIGH     = Number(__ENV.RPS_HIGH     || 500);
const RPS_STRESS   = Number(__ENV.RPS_STRESS   || 1000);
const RPS_PEAK     = Number(__ENV.RPS_PEAK     || 1500);
const FEATURES_RPS = Number(__ENV.FEATURES_RPS || 200);

export const options = {
    scenarios: {
        post_metrics: {
            executor: 'ramping-arrival-rate',
            exec: 'postMetrics',
            startRate: RPS_WARMUP,
            timeUnit: '1s',
            preAllocatedVUs: 100,
            maxVUs: 3000,
            stages: [
                { duration: '1m',  target: RPS_WARMUP },
                { duration: '2m',  target: RPS_NORMAL },
                { duration: '2m',  target: RPS_HIGH },
                { duration: '2m',  target: RPS_STRESS },
                { duration: '2m',  target: RPS_PEAK },
                { duration: '1m',  target: 0 },
            ],
        },
        get_features: {
            executor: 'constant-arrival-rate',
            exec: 'getFeatures',
            rate: FEATURES_RPS,
            timeUnit: '1s',
            duration: '10m',
            preAllocatedVUs: 50,
            maxVUs: 500,
        },
    },
    thresholds: {
        http_req_failed: ['rate<0.05'],
        'http_req_duration{scenario:post_metrics}': ['p(95)<500', 'p(99)<1000'],
        'http_req_duration{scenario:get_features}': ['p(95)<50'],
        metrics_errors: ['count<100'],
        features_errors: ['count<100'],
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

    const impactMetricsAmount = 50

    return JSON.stringify({
        appName: `stress-test-app-${vuId % 10}`,
        instanceId: `instance-${vuId}`,
        bucket: {
            start: start.toISOString(),
            stop: now.toISOString(),
            toggles: features,
        },
        impactMetrics: Array.from({ length: impactMetricsAmount }, (_, i) =>
            i % 2 === 0
                ? {
                      name: `stress_counter_${i}_${vuId % 5}`,
                      type: 'counter',
                      help: 'A stress test counter',
                      samples: [
                          {
                              value: Math.floor(Math.random() * 1000),
                              labels: { appName: `stress-test-app-${vuId % 10}` },
                          },
                      ],
                  }
                : {
                      name: `stress_histogram_${i}_${vuId % 3}`,
                      type: 'histogram',
                      help: 'A stress test histogram',
                      samples: [
                          {
                              labels: { appName: `stress-test-app-${vuId % 10}` },
                              count: Math.floor(Math.random() * 500) + 10,
                              sum: Math.random() * 5000,
                              buckets: [
                                  { le: 1.0,      count: Math.floor(Math.random() * 50) },
                                  { le: 5.0,      count: Math.floor(Math.random() * 200) },
                                  { le: 10.0,     count: Math.floor(Math.random() * 400) },
                                  { le: '+Inf',   count: Math.floor(Math.random() * 500) },
                              ],
                          },
                      ],
                  }
        ),
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
