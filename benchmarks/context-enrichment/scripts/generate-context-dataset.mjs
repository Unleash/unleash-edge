#!/usr/bin/env node
// Generates an immutable SQLite snapshot for the context-dataset proof of concept.
// Requires Node 22+ (node:sqlite) and zstd on PATH; neither is an Enterprise dependency.

import { DatabaseSync } from 'node:sqlite';
import { createHash } from 'node:crypto';
import { createReadStream, mkdirSync, renameSync, rmSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const values = Object.fromEntries(
    process.argv.slice(2).map((value, index, args) =>
        value.startsWith('--') ? [value.slice(2), args[index + 1]] : [],
    ).filter(([key]) => key),
);
const rows = Number(values.rows ?? 1_000);
const revision = Number(values.revision ?? 1);
const output = resolve(values.output ?? './context-datasets');
const profile = values.profile ?? 'repetitive';

if (!Number.isSafeInteger(rows) || rows < 1 || !Number.isSafeInteger(revision) || revision < 1) {
    throw new Error('--rows and --revision must be positive integers');
}
if (!['repetitive', 'high-entropy'].includes(profile)) {
    throw new Error('--profile must be repetitive or high-entropy');
}

const deterministicHex = (row, field) => createHash('sha256')
    .update(`${revision}:${row}:${field}`)
    .digest('hex');

const attributesFor = (row) => profile === 'high-entropy'
    ? {
        cohort: row % 10,
        country: ['ES', 'NO', 'US', 'GB'][row % 4],
        color: ['blue', 'green'][row % 2],
        account_key: deterministicHex(row, 'account'),
        request_key: deterministicHex(row, 'request'),
        email: `${deterministicHex(row, 'email').slice(0, 20)}@example.test`,
    }
    : {
        cohort: row % 10,
        country: ['ES', 'NO', 'US', 'GB'][row % 4],
        color: ['blue', 'green'][row % 2],
    };

mkdirSync(output, { recursive: true });
const sqliteFile = resolve(output, `dataset-${revision}.sqlite`);
const compressedFile = `${sqliteFile}.zst`;
rmSync(sqliteFile, { force: true });
rmSync(compressedFile, { force: true });

const database = new DatabaseSync(sqliteFile);
database.exec(`
    PRAGMA journal_mode = OFF;
    PRAGMA synchronous = OFF;
    CREATE TABLE context_data (
        subject_id TEXT PRIMARY KEY,
        segment_id TEXT NOT NULL,
        attributes_json TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE INDEX context_data_segment_subject ON context_data(segment_id, subject_id);
`);
const insert = database.prepare(
    'INSERT INTO context_data (subject_id, segment_id, attributes_json) VALUES (?, ?, ?)',
);
database.exec('BEGIN');
for (let row = 0; row < rows; row += 1) {
    const subjectId = `subject-${String(row).padStart(12, '0')}`;
    insert.run(subjectId, `segment-${row % 100}`, JSON.stringify(attributesFor(row)));
}
database.exec('COMMIT; VACUUM;');
database.close();

const compression = spawnSync('zstd', ['-q', '-f', '-o', compressedFile, sqliteFile], {
    stdio: 'inherit',
});
if (compression.status !== 0) {
    throw new Error('zstd compression failed');
}

const checksum = createHash('sha256');
let size = 0;
for await (const chunk of createReadStream(sqliteFile)) {
    checksum.update(chunk);
    size += chunk.length;
}
const manifest = {
    revision,
    schemaVersion: 1,
    file: basename(compressedFile),
    sha256: checksum.digest('hex'),
    size,
    profile,
};
const temporaryManifest = resolve(output, 'manifest.json.tmp');
await writeFile(temporaryManifest, JSON.stringify(manifest, null, 2));
renameSync(temporaryManifest, resolve(output, 'manifest.json'));
console.log(JSON.stringify(manifest, null, 2));
