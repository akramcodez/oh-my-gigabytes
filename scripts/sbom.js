#!/usr/bin/env node
'use strict';
const { spawnSync } = require('node:child_process');
const crypto = require('node:crypto');

// Produces a local dependency inventory; never publishes or uploads it.
const result = spawnSync('cargo', ['metadata', '--locked', '--format-version', '1'], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
if (result.status !== 0) {
  process.stderr.write(result.stderr || 'cargo metadata failed\n');
  process.exitCode = 1;
} else {
  const metadata = JSON.parse(result.stdout);
  const document = {
    bomFormat: 'CycloneDX', specVersion: '1.5', version: 1,
    metadata: { timestamp: new Date().toISOString(), tools: [{ name: 'omg-local-dependency-inventory', version: '1' }] },
    components: metadata.packages.map(pkg => ({ type: 'library', name: pkg.name, version: pkg.version,
      'bom-ref': crypto.createHash('sha256').update(pkg.id).digest('hex'),
      licenses: pkg.license ? [{ expression: pkg.license }] : [],
      properties: [{ name: 'omg:source', value: pkg.source || 'workspace' }] })),
    dependencies: metadata.resolve.nodes.map(node => ({ ref: crypto.createHash('sha256').update(node.id).digest('hex'),
      dependsOn: node.dependencies.map(id => crypto.createHash('sha256').update(id).digest('hex')) })),
  };
  process.stdout.write(JSON.stringify(document, null, 2) + '\n');
}
