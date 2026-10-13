#!/usr/bin/env node
'use strict';

const crypto = require('node:crypto');
const { parseArgs } = require('node:util');
const { readRegular, verifyArtifact } = require('../packages/oh-my-gigabytes/lib/verify');

try {
  const { values } = parseArgs({ options: {
    artifact: { type: 'string' }, 'public-key': { type: 'string' },
    target: { type: 'string' }, version: { type: 'string' },
  } });
  for (const key of ['artifact', 'public-key', 'target', 'version']) if (!values[key]) throw new Error(`--${key} required`);
  // This key must already be trusted independently; do not select it from the downloaded artifact.
  const key = crypto.createPublicKey(readRegular(values['public-key'], 16 * 1024).data);
  const keyId = crypto.createHash('sha256').update(key.export({ type: 'spki', format: 'der' })).digest('hex');
  const trust = { schema: 1, keys: { [keyId]: key.export({ type: 'spki', format: 'pem' }) } };
  const result = verifyArtifact(values.artifact, trust, { target: values.target, version: values.version });
  process.stdout.write(`Verified omg ${result.manifest.version} ${result.manifest.triple}; key ${keyId}\n`);
} catch (error) { process.stderr.write(`omg verify: ${error.message}\n`); process.exitCode = 1; }
