#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { hostTarget, readRegular, verifyArtifact } = require('../lib/verify');

function resolveArtifact(root, target, version) {
  const vendor = path.join(root, 'vendor', target);
  if (fs.existsSync(vendor)) return vendor;
  const name = `oh-my-gigabytes-${target}`;
  let packagePath;
  try { packagePath = require.resolve(`${name}/package.json`, { paths: [root] }); }
  catch { throw new Error(`Missing native package ${name}@${version}. Optional dependencies may have been omitted. Reinstall with optional dependencies or use the signed offline bundle; no binary will be downloaded by omg.`); }
  const metadata = JSON.parse(readRegular(packagePath, 16 * 1024).data.toString('utf8'));
  if (metadata.name !== name || metadata.version !== version) throw new Error('Native package version/name mismatch');
  return path.join(path.dirname(packagePath), 'artifact');
}

function main() {
  const root = path.resolve(__dirname, '..');
  const pkg = require('../package.json');
  const target = hostTarget();
  const trust = JSON.parse(readRegular(path.join(root, 'trust.json'), 64 * 1024).data.toString('utf8'));
  const artifact = resolveArtifact(root, target, pkg.version);
  const { binary } = verifyArtifact(artifact, trust, { target, version: pkg.version });
  // Trust is rooted in the installed wrapper. Installation directories must remain user-controlled.
  // Reverify every launch, never accept a cached digest for a replaced executable.
  const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit', shell: false });
  const handlers = new Map();
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    const handler = () => { if (child.exitCode === null) child.kill(signal); };
    handlers.set(signal, handler);
    process.on(signal, handler);
  }
  const cleanup = () => { for (const [signal, handler] of handlers) process.removeListener(signal, handler); };
  child.once('error', error => { cleanup(); process.stderr.write(`omg: native launch failed: ${error.message}\n`); process.exitCode = 1; });
  child.once('exit', (code, signal) => {
    cleanup();
    process.exitCode = code ?? ({ SIGINT: 130, SIGTERM: 143, SIGHUP: 129 }[signal] || 1);
  });
}

if (require.main === module) {
  try { main(); } catch (error) { process.stderr.write(`omg: ${error.message}\n`); process.exitCode = 1; }
}

module.exports = { main, resolveArtifact };
