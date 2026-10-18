#!/usr/bin/env node
'use strict';

// Turns the unsigned binaries built by .github/workflows/release.yml into signed packages and
// tarballs (release plan R19). Runs on the owner's machine with the offline Ed25519 key; the key
// never goes to CI. Nothing here uploads or publishes anything.
//
//   node scripts/sign-release.js --artifacts DIR --version 0.1.0-beta.1 \
//     --public-key release-public.pem --private-key release-private.pem --out OUT \
//     [--repo owner/name] [--skip-attestation] [--publishable]
//
// DIR is the output of `gh run download RUN_ID`: DIR/omg-<target>/omg (or omg.exe) + SHA256SUMS.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const { parseArgs } = require('node:util');
const { TARGETS } = require('../packages/oh-my-gigabytes/lib/verify');
const { assemble } = require('./assemble-release');

const sha256 = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');

// On Windows npm is a .cmd shim that Node will not spawn directly; run its script with node instead.
function npmCommand() {
  const script = path.join(path.dirname(process.execPath), 'node_modules', 'npm', 'bin', 'npm-cli.js');
  return process.platform === 'win32' && fs.existsSync(script) ? [process.execPath, [script]] : ['npm', []];
}

function run(command, args, options = {}) {
  if (command === 'npm') {
    const [executable, prefix] = npmCommand();
    command = executable;
    args = [...prefix, ...args];
  }
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  if (result.error || result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed: ${result.error?.message || result.stderr || result.stdout}`);
  }
  return result.stdout;
}

// The build recorded `<sha256>  <file>`; the downloaded binary must still match it.
function checkDigest(directory, filename) {
  const listed = fs.readFileSync(path.join(directory, 'SHA256SUMS'), 'utf8').trim().split(/\s+/)[0];
  if (listed !== sha256(path.join(directory, filename))) throw new Error(`${filename} does not match its SHA256SUMS`);
}

function pack(directory, destination, expectedName) {
  const output = JSON.parse(run('npm', ['pack', '--ignore-scripts', '--json', '--pack-destination', destination], { cwd: directory }));
  const produced = path.join(destination, output[0].filename);
  const target = path.join(destination, expectedName);
  if (produced !== target) fs.renameSync(produced, target);
  return target;
}

function signRelease({ artifacts, version, publicKey, privateKey, out, repo, skipAttestation = false, publishable = false, checkBinary = true }) {
  if (!skipAttestation && !repo) throw new Error('--repo owner/name is required to verify build provenance (or pass --skip-attestation)');
  const targets = Object.keys(TARGETS).filter(target => fs.existsSync(path.join(artifacts, `omg-${target}`)));
  if (!targets.length) throw new Error(`No omg-<target> folders found in ${artifacts}`);
  if (fs.existsSync(out)) throw new Error('Output directory already exists; refuse to overwrite a release');
  fs.mkdirSync(out, { mode: 0o700 });
  const work = path.join(out, 'work');
  const tarballs = path.join(out, 'tarballs');
  const offline = path.join(out, 'offline');
  for (const directory of [work, tarballs, offline]) fs.mkdirSync(directory);

  const natives = [];
  let keyId;
  for (const target of targets) {
    const directory = path.join(artifacts, `omg-${target}`);
    const filename = target.startsWith('win32-') ? 'omg.exe' : 'omg';
    const binary = path.join(directory, filename);
    checkDigest(directory, filename);
    if (!skipAttestation) run('gh', ['attestation', 'verify', binary, '--repo', repo]);
    const assembled = path.join(work, target);
    ({ keyId } = assemble({ binary, target, version, publicKey, privateKey, out: assembled, variant: 'analysis-only', publishable, checkBinary }));
    pack(path.join(assembled, `oh-my-gigabytes-${target}`), tarballs, `oh-my-gigabytes-${target}-${version}.tgz`);
    pack(path.join(assembled, 'offline-oh-my-gigabytes'), offline, `oh-my-gigabytes-offline-${target}-${version}.tgz`);
    natives.push(target);
  }

  // The published wrapper names every platform package. Each per-target wrapper only names its own.
  const wrapper = path.join(work, natives[0], 'oh-my-gigabytes');
  const manifest = JSON.parse(fs.readFileSync(path.join(wrapper, 'package.json'), 'utf8'));
  manifest.optionalDependencies = Object.fromEntries(natives.map(target => [`oh-my-gigabytes-${target}`, version]));
  fs.writeFileSync(path.join(wrapper, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');
  pack(wrapper, tarballs, `oh-my-gigabytes-${version}.tgz`);

  fs.copyFileSync(publicKey, path.join(out, 'release-public.pem'));
  fs.writeFileSync(path.join(out, 'KEY-FINGERPRINT.txt'), `${keyId}\n`);
  const files = [...fs.readdirSync(tarballs).map(name => path.join('tarballs', name)), ...fs.readdirSync(offline).map(name => path.join('offline', name))].sort();
  // Plain file names, so `sha256sum -c SHA256SUMS` works in a folder holding the downloaded assets.
  fs.writeFileSync(path.join(out, 'SHA256SUMS'), files.map(file => `${sha256(path.join(out, file))}  ${path.basename(file)}`).join('\n') + '\n');
  fs.rmSync(work, { recursive: true, force: true });
  return { version, targets: natives, keyId, files, out };
}

if (require.main === module) {
  try {
    const { values } = parseArgs({ options: {
      artifacts: { type: 'string' }, version: { type: 'string' }, 'public-key': { type: 'string' },
      'private-key': { type: 'string' }, out: { type: 'string' }, repo: { type: 'string' },
      'skip-attestation': { type: 'boolean' }, publishable: { type: 'boolean' },
    } });
    for (const field of ['artifacts', 'version', 'public-key', 'private-key', 'out']) if (!values[field]) throw new Error(`--${field} required`);
    const result = signRelease({ artifacts: values.artifacts, version: values.version, publicKey: values['public-key'], privateKey: values['private-key'],
      out: values.out, repo: values.repo, skipAttestation: values['skip-attestation'] === true, publishable: values.publishable === true });
    process.stdout.write(JSON.stringify(result, null, 2) + '\n');
  } catch (error) { process.stderr.write(`omg release: ${error.message}\n`); process.exitCode = 1; }
}
module.exports = { signRelease };
