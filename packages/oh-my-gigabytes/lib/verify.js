'use strict';

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const DOMAIN = Buffer.from('OMG-BINARY-MANIFEST-V1\0');
const MAX_BINARY = 128 * 1024 * 1024;
const MAX_MANIFEST = 16 * 1024;
const TARGETS = Object.freeze({
  'linux-x64-gnu': 'x86_64-unknown-linux-gnu',
  'linux-arm64-gnu': 'aarch64-unknown-linux-gnu',
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'win32-x64': 'x86_64-pc-windows-msvc',
});

function exactKeys(value, fields, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value) ||
      Object.keys(value).sort().join(',') !== [...fields].sort().join(',')) {
    throw new Error(`Invalid ${label} fields`);
  }
}

function hostTarget(host = process) {
  const name = `${host.platform}-${host.arch}`;
  if (host.platform === 'linux') {
    // GNU-only for now. Never guess glibc compatibility from an executable name.
    const glibc = host.report?.getReport().header.glibcVersionRuntime;
    if (!glibc) throw new Error('Unsupported Linux libc: a GNU/glibc build is required; no musl binary is published');
    const target = `${name}-gnu`;
    if (TARGETS[target]) return target;
  } else if (TARGETS[name]) return name;
  throw new Error(`Unsupported platform ${name}; see docs/release.md for available targets`);
}

function signedMessage(manifestBytes) {
  return Buffer.concat([DOMAIN, manifestBytes]);
}

function validateBinaryTarget(data, target) {
  if (target.startsWith('linux-')) {
    const machine = target.includes('-arm64-') ? 183 : 62;
    if (data.length < 64 || data.subarray(0, 4).toString('hex') !== '7f454c46' ||
        data[4] !== 2 || data[5] !== 1 || data.readUInt16LE(18) !== machine) {
      throw new Error('Binary format/architecture mismatch (ELF64 required)');
    }
  } else if (target.startsWith('win32-')) {
    if (data.length < 64 || data.subarray(0, 2).toString() !== 'MZ') throw new Error('Binary format mismatch (PE required)');
    const offset = data.readUInt32LE(60);
    if (offset + 6 > data.length || data.subarray(offset, offset + 4).toString('hex') !== '50450000' || data.readUInt16LE(offset + 4) !== 0x8664) {
      throw new Error('Binary format/architecture mismatch (x64 PE required)');
    }
  } else {
    const cpu = target.endsWith('arm64') ? 0x0100000c : 0x01000007;
    if (data.length < 32 || data.readUInt32LE(0) !== 0xfeedfacf || data.readUInt32LE(4) !== cpu) {
      throw new Error('Binary format/architecture mismatch (thin 64-bit Mach-O required)');
    }
  }
}

function readRegular(file, maximum) {
  const metadata = fs.lstatSync(file);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > maximum) {
    throw new Error('Artifact must be a bounded regular file; redirection is not permitted');
  }
  const flags = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0);
  const fd = fs.openSync(file, flags);
  try {
    const opened = fs.fstatSync(fd);
    if (!opened.isFile() || opened.size > maximum || opened.dev !== metadata.dev || opened.ino !== metadata.ino) {
      throw new Error('Artifact changed during verification');
    }
    // Read only expected bytes plus one; do not permit concurrent growth to allocate unbounded RAM.
    const data = Buffer.alloc(opened.size + 1);
    let count = 0;
    while (count < data.length) {
      const n = fs.readSync(fd, data, count, data.length - count, null);
      if (!n) break;
      count += n;
    }
    const after = fs.fstatSync(fd);
    if (count !== opened.size || after.size !== opened.size || after.mtimeMs !== opened.mtimeMs || after.ctimeMs !== opened.ctimeMs) {
      throw new Error('Artifact changed during verification');
    }
    return { data: data.subarray(0, count), metadata: after };
  } finally {
    fs.closeSync(fd);
  }
}

function verifyArtifact(directory, trust, expected) {
  exactKeys(trust, ['schema', 'keys'], 'trust document');
  if (trust.schema !== 1 || !trust.keys || typeof trust.keys !== 'object' || Array.isArray(trust.keys)) {
    throw new Error('Unsupported trust document');
  }
  if (!Object.keys(trust.keys).length) {
    throw new Error('No release signing key provisioned: this source wrapper cannot launch an unverified binary');
  }
  if (!TARGETS[expected.target]) throw new Error('Unsupported expected target');
  const root = fs.lstatSync(directory);
  if (!root.isDirectory() || root.isSymbolicLink()) throw new Error('Artifact directory cannot be redirected');
  const manifestBytes = readRegular(path.join(directory, 'manifest.json'), MAX_MANIFEST).data;
  const signature = readRegular(path.join(directory, 'manifest.sig'), 64).data;
  if (signature.length !== 64) throw new Error('Invalid release signature length');
  const manifest = JSON.parse(manifestBytes.toString('utf8'));
  exactKeys(manifest, ['schema', 'version', 'target', 'triple', 'file', 'size', 'sha256', 'keyId', 'capabilities'], 'manifest');
  const keyText = trust.keys[manifest.keyId];
  if (typeof keyText !== 'string') throw new Error('Release signing key is not trusted');
  const key = crypto.createPublicKey(keyText);
  if (key.asymmetricKeyType !== 'ed25519' || !crypto.verify(null, signedMessage(manifestBytes), key, signature)) {
    throw new Error('Release authenticity verification failed');
  }
  if (manifest.schema !== 1 || manifest.version !== expected.version || manifest.target !== expected.target ||
      manifest.triple !== TARGETS[expected.target]) throw new Error('Release version/target/schema mismatch');
  const file = expected.target.startsWith('win32-') ? 'omg.exe' : 'omg';
  if (manifest.file !== file || !Number.isSafeInteger(manifest.size) || manifest.size <= 0 || manifest.size > MAX_BINARY ||
      !/^[a-f0-9]{64}$/.test(manifest.sha256)) throw new Error('Invalid binary scope/size/digest');
  exactKeys(manifest.capabilities, ['analysis', 'cleanup', 'service'], 'capabilities');
  const linux = expected.target.startsWith('linux-');
  // Linux: permanent delete is `development-unqualified` (full build) or `disabled` (analysis-only build).
  const cleanupValues = linux ? ['development-unqualified', 'disabled'] : ['disabled'];
  if (manifest.capabilities.analysis !== (linux ? 'development-linux' : 'disabled') ||
      !cleanupValues.includes(manifest.capabilities.cleanup) ||
      manifest.capabilities.service !== (linux ? 'development-linux' : 'disabled')) {
    throw new Error('Unsupported advertised capabilities');
  }
  const binary = path.join(directory, file);
  const { data, metadata } = readRegular(binary, MAX_BINARY);
  if (data.length !== manifest.size || crypto.createHash('sha256').update(data).digest('hex') !== manifest.sha256) {
    throw new Error('Binary integrity verification failed');
  }
  validateBinaryTarget(data, expected.target);
  if (process.platform !== 'win32' && (metadata.mode & 0o022)) {
    throw new Error('Binary cannot be group/world writable; reinstall into a trusted user-controlled location');
  }
  return { binary, manifest, metadata };
}

module.exports = { DOMAIN, MAX_BINARY, MAX_MANIFEST, TARGETS, hostTarget, readRegular, signedMessage, validateBinaryTarget, verifyArtifact };
