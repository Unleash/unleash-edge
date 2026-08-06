#!/usr/bin/env node
// Mock hot-path context enricher for the Unleash Edge enrichment benchmark.
//
// Contract mirrors what unleash-edge's `enrich_context` expects:
//   - Edge POSTs the camelCase Context JSON (at least { "userId": "..." }).
//   - We look userId up as `subject_id` in the SAME SQLite dataset the cold-path
//     POC generates, and return a FLAT { key: "value" } object of string values.
//   - Unknown subject -> 200 with {} (Edge treats this as "nothing to add").
//
// Values are stringified because Edge deserialises the response as
// HashMap<String, String>; a numeric JSON value would fail that decode.
//
// Resilience knobs (env), so we can reproduce the cold POC's failure scenarios:
//   INJECT_LATENCY_MS  fixed added delay per request           (default 0)
//   ERROR_RATE         fraction of requests answered with 500  (default 0)
//   DATASET_FILE       path to an uncompressed dataset-N.sqlite (required)
//   PORT               listen port                             (default 8080)

import { createServer } from 'node:http';
import { DatabaseSync } from 'node:sqlite';

const datasetFile = process.env.DATASET_FILE;
const port = Number(process.env.PORT ?? 8080);
const injectLatencyMs = Number(process.env.INJECT_LATENCY_MS ?? 0);
const errorRate = Number(process.env.ERROR_RATE ?? 0);

if (!datasetFile) {
    throw new Error('DATASET_FILE is required (path to an uncompressed dataset-N.sqlite)');
}

// One read-only connection reused for the process lifetime — the enricher is the
// dependency under test, so we do not want its own DB setup to be the bottleneck.
const database = new DatabaseSync(datasetFile, { readOnly: true });
const lookup = database.prepare(
    'SELECT attributes_json FROM context_data WHERE subject_id = ?',
);

const stringifyValues = (attributes) =>
    Object.fromEntries(Object.entries(attributes).map(([key, value]) => [key, String(value)]));

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const readBody = (request) =>
    new Promise((resolve, reject) => {
        const chunks = [];
        request.on('data', (chunk) => chunks.push(chunk));
        request.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
        request.on('error', reject);
    });

const server = createServer(async (request, response) => {
    if (request.method === 'GET' && request.url === '/health') {
        response.writeHead(200).end('ok');
        return;
    }
    if (request.method !== 'POST') {
        response.writeHead(405).end();
        return;
    }

    if (injectLatencyMs > 0) await sleep(injectLatencyMs);
    if (errorRate > 0 && Math.random() < errorRate) {
        response.writeHead(500).end('{"error":"injected"}');
        return;
    }

    let subjectId;
    try {
        subjectId = JSON.parse(await readBody(request))?.userId;
    } catch {
        response.writeHead(400).end('{"error":"invalid json"}');
        return;
    }

    let attributes = {};
    if (typeof subjectId === 'string') {
        const row = lookup.get(subjectId);
        if (row) attributes = stringifyValues(JSON.parse(row.attributes_json));
    }
    response.writeHead(200, { 'Content-Type': 'application/json' }).end(JSON.stringify(attributes));
});

server.listen(port, () => {
    console.log(
        JSON.stringify({
            msg: 'enricher listening',
            port,
            datasetFile,
            injectLatencyMs,
            errorRate,
        }),
    );
});
