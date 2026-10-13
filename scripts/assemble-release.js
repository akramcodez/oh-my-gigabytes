#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { parseArgs } = require('node:util');
const { TARGETS, MAX_BINARY, readRegular, signedMessage, validateBinaryTarget, verifyArtifact } = require('../packages/oh-my-gigabytes/lib/verify');

function assemble({ binary, target, version, publicKey, privateKey, out }) {
  if (!TARGETS[target]) throw new Error('Unsupported target');
  if (!/^\d+\.\d+\.\d+(?:-[a-z0-9.-]+)?$/.test(version)) throw new Error('Invalid release version');
  if (fs.existsSync(out)) throw new Error('Output directory already exists; refuse to overwrite a release');
  const parent = path.dirname(path.resolve(out));
  if (!fs.statSync(parent).isDirectory()) throw new Error('Output parent must already exist');
  const privateMetadata = fs.lstatSync(privateKey);
  if (process.platform !== 'win32' && (privateMetadata.mode & 0o077)) throw new Error('Signing key must have private (0600) permissions');
  const key = crypto.createPrivateKey(readRegular(privateKey, 16 * 1024).data);
  const trusted = crypto.createPublicKey(readRegular(publicKey, 16 * 1024).data);
  if (key.asymmetricKeyType !== 'ed25519' || trusted.asymmetricKeyType !== 'ed25519') throw new Error('Ed25519 release keys required');
  const publicDer = trusted.export({ type: 'spki', format: 'der' });
  if (!publicDer.equals(crypto.createPublicKey(key).export({ type: 'spki', format: 'der' }))) throw new Error('Signing key does not match maintainer-selected public trust root');
  const data = readRegular(binary, MAX_BINARY).data;
  if (!data.length) throw new Error('Empty binary');
  validateBinaryTarget(data, target);
  const keyId = crypto.createHash('sha256').update(publicDer).digest('hex');
  const trust = { schema: 1, keys: { [keyId]: trusted.export({ type: 'spki', format: 'pem' }) } };
  const filename = target.startsWith('win32-') ? 'omg.exe' : 'omg';
  const manifest = { schema: 1, version, target, triple: TARGETS[target], file: filename,
    size: data.length, sha256: crypto.createHash('sha256').update(data).digest('hex'), keyId,
    capabilities: { analysis: target.startsWith('linux-') ? 'development-linux' : 'disabled', cleanup: 'disabled', service: target.startsWith('linux-') ? 'development-linux' : 'disabled' } };
  const bytes = Buffer.from(JSON.stringify(manifest) + '\n');
  const signature = crypto.sign(null, signedMessage(bytes), key);
  const stage = fs.mkdtempSync(path.join(parent, '.omg-release-'));
  const source = path.resolve(__dirname, '../packages/oh-my-gigabytes');
  try {
    const native = path.join(stage, `oh-my-gigabytes-${target}`);
    const artifact = path.join(native, 'artifact');
    fs.mkdirSync(artifact, { recursive: true, mode: 0o700 });
    fs.writeFileSync(path.join(artifact, filename), data, { mode: 0o755 });
    fs.writeFileSync(path.join(artifact, 'manifest.json'), bytes);
    fs.writeFileSync(path.join(artifact, 'manifest.sig'), signature);
    const nativePackage = { name: `oh-my-gigabytes-${target}`, version, private: true,
      license: 'MIT OR Apache-2.0', os: [target.split('-')[0]], cpu: [target.split('-')[1]],
      files: ['artifact', 'LICENSE-MIT', 'LICENSE-APACHE', 'README.md'] };
    if (target.startsWith('linux-')) nativePackage.libc = ['glibc'];
    fs.writeFileSync(path.join(native, 'package.json'), JSON.stringify(nativePackage, null, 2) + '\n');
    for (const offline of [false, true]) {
      const wrapper = path.join(stage, offline ? 'offline-oh-my-gigabytes' : 'oh-my-gigabytes');
      fs.mkdirSync(wrapper);
      for (const name of ['bin', 'lib', 'README.md']) fs.cpSync(path.join(source, name), path.join(wrapper, name), { recursive: true });
      fs.chmodSync(path.join(wrapper, 'bin/omg.js'), 0o755);
      const pkg = JSON.parse(fs.readFileSync(path.join(source, 'package.json'), 'utf8'));
      delete pkg.scripts;
      pkg.version = version;
      if (offline) {
        pkg.os = nativePackage.os; pkg.cpu = nativePackage.cpu;
        if (nativePackage.libc) pkg.libc = nativePackage.libc;
        fs.mkdirSync(path.join(wrapper, 'vendor'), { recursive: true });
        fs.cpSync(artifact, path.join(wrapper, 'vendor', target), { recursive: true });
      } else pkg.optionalDependencies = { [nativePackage.name]: version };
      fs.writeFileSync(path.join(wrapper, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');
      fs.writeFileSync(path.join(wrapper, 'trust.json'), JSON.stringify(trust, null, 2) + '\n');
      for (const license of ['LICENSE-MIT', 'LICENSE-APACHE']) fs.copyFileSync(path.resolve(__dirname, '..', license), path.join(wrapper, license));
    }
    for (const name of ['LICENSE-MIT', 'LICENSE-APACHE']) fs.copyFileSync(path.resolve(__dirname, '..', name), path.join(native, name));
    fs.copyFileSync(path.join(source, 'README.md'), path.join(native, 'README.md'));
    verifyArtifact(artifact, trust, { target, version });
    fs.renameSync(stage, out);
    return { keyId, target, version, out };
  } catch (error) {
    fs.rmSync(stage, { recursive: true, force: true });
    throw error;
  }
}

if (require.main === module) {
  try {
    const { values } = parseArgs({ options: {
      binary: { type: 'string' }, target: { type: 'string' }, version: { type: 'string' },
      'public-key': { type: 'string' }, 'private-key': { type: 'string' }, out: { type: 'string' },
    } });
    for (const field of ['binary', 'target', 'version', 'public-key', 'private-key', 'out']) if (!values[field]) throw new Error(`--${field} required`);
    process.stdout.write(JSON.stringify(assemble({ ...values, publicKey: values['public-key'], privateKey: values['private-key'] })) + '\n');
  } catch (error) { process.stderr.write(`omg release: ${error.message}\n`); process.exitCode = 1; }
}
module.exports = { assemble };
