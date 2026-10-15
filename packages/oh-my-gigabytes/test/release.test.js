'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const { assemble } = require('../../../scripts/assemble-release');
const { hostTarget, signedMessage, verifyArtifact } = require('../lib/verify');
const { resolveArtifact } = require('../bin/omg');

function fixture(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'omg-package-test-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const publicFile = path.join(directory, 'release-public.pem');
  const privateFile = path.join(directory, 'release-private.pem');
  fs.writeFileSync(publicFile, publicKey.export({ type: 'spki', format: 'pem' }));
  fs.writeFileSync(privateFile, privateKey.export({ type: 'pkcs8', format: 'pem' }), { mode: 0o600 });
  const binary = path.join(directory, 'fixture-binary');
  const elf = Buffer.alloc(64);
  Buffer.from('7f454c46', 'hex').copy(elf);
  elf[4] = 2; elf[5] = 1; elf.writeUInt16LE(62, 18);
  fs.writeFileSync(binary, elf, { mode: 0o755 });
  const target = 'linux-x64-gnu';
  const version = '0.1.0';
  const out = path.join(directory, 'release');
  // The fixture binary is not executable, so the doctor cross-check is skipped for it.
  assemble({ binary, target, version, publicKey: publicFile, privateKey: privateFile, out, checkBinary: false });
  const artifact = path.join(out, `oh-my-gigabytes-${target}`, 'artifact');
  const trust = JSON.parse(fs.readFileSync(path.join(out, 'oh-my-gigabytes/trust.json'), 'utf8'));
  return { directory, publicKey, privateKey, publicFile, privateFile, binary, target, version, out, artifact, trust };
}

function resign(f, change) {
  const file = path.join(f.artifact, 'manifest.json');
  const manifest = JSON.parse(fs.readFileSync(file, 'utf8'));
  change(manifest);
  const data = Buffer.from(JSON.stringify(manifest) + '\n');
  fs.writeFileSync(file, data);
  fs.writeFileSync(path.join(f.artifact, 'manifest.sig'), crypto.sign(null, signedMessage(data), f.privateKey));
}

test('analysis-only variant signs cleanup as disabled and verifies', t => {
  const f = fixture(t);
  const out = path.join(f.directory, 'release-analysis');
  assemble({ binary: f.binary, target: f.target, version: f.version, publicKey: f.publicFile, privateKey: f.privateFile, out, variant: 'analysis-only', checkBinary: false });
  const artifact = path.join(out, `oh-my-gigabytes-${f.target}`, 'artifact');
  const trust = JSON.parse(fs.readFileSync(path.join(out, 'oh-my-gigabytes/trust.json'), 'utf8'));
  const result = verifyArtifact(artifact, trust, { target: f.target, version: f.version });
  assert.equal(result.manifest.capabilities.cleanup, 'disabled');
});

test('a real build cannot be signed with a capability its own doctor does not report', t => {
  const real = path.resolve(__dirname, '../../../target/debug/omg');
  if (process.platform !== 'linux' || !fs.existsSync(real) || process.arch !== 'x64') t.skip('needs a runnable x86_64 Linux omg binary');
  const f = fixture(t);
  const out = path.join(f.directory, 'release-mislabel');
  assert.throws(() => assemble({ binary: real, target: f.target, version: f.version, publicKey: f.publicFile, privateKey: f.privateFile, out, variant: 'analysis-only' }), /reports cleanup/);
  assert.equal(fs.existsSync(out), false, 'nothing is written when the claim is wrong');
});

test('signed assembly verifies and offline payload contains no private signing material', t => {
  const f = fixture(t);
  const result = verifyArtifact(f.artifact, f.trust, f);
  assert.equal(result.manifest.version, f.version);
  const target = result.manifest.target;
  const native = target.startsWith('linux-') || target.startsWith('win32-');
  assert.equal(result.manifest.capabilities.cleanup, native ? 'development-unqualified' : 'disabled');
  assert.equal(result.manifest.capabilities.analysis,
    target.startsWith('linux-') ? 'development-linux' : target.startsWith('win32-') ? 'development-windows' : 'disabled');
  assert.equal(result.manifest.capabilities.service, target.startsWith('linux-') ? 'development-linux' : 'disabled');
  const offline = path.join(f.out, 'offline-oh-my-gigabytes');
  assert.equal(resolveArtifact(offline, f.target, f.version), path.join(offline, 'vendor', f.target));
  assert.ok(!fs.readFileSync(path.join(offline, 'trust.json'), 'utf8').includes('PRIVATE KEY'));
  assert.throws(() => assemble({ binary: f.binary, target: f.target, version: f.version,
    publicKey: f.publicFile, privateKey: f.privateFile, out: f.out }), /already exists/);
});

test('changed executable is detected on every verification', t => {
  const f = fixture(t);
  verifyArtifact(f.artifact, f.trust, f);
  fs.writeFileSync(path.join(f.artifact, 'omg'), 'changed binary');
  assert.throws(() => verifyArtifact(f.artifact, f.trust, f), /integrity/);
});

test('even authenticated bytes must match the native executable architecture', t => {
  const f = fixture(t);
  const binary = path.join(f.artifact, 'omg');
  const data = fs.readFileSync(binary);
  data.writeUInt16LE(183, 18); // ARM64 ELF cannot be shipped as x86_64.
  fs.writeFileSync(binary, data);
  resign(f, m => { m.sha256 = crypto.createHash('sha256').update(data).digest('hex'); });
  assert.throws(() => verifyArtifact(f.artifact, f.trust, f), /architecture mismatch/);
});

test('tampered signature and wrong trust root fail closed', t => {
  const f = fixture(t);
  const wrong = crypto.generateKeyPairSync('ed25519').publicKey;
  const trust = { schema: 1, keys: { [Object.keys(f.trust.keys)[0]]: wrong.export({ type: 'spki', format: 'pem' }) } };
  assert.throws(() => verifyArtifact(f.artifact, trust, f), /authenticity/);
  fs.writeFileSync(path.join(f.artifact, 'manifest.sig'), Buffer.alloc(64));
  assert.throws(() => verifyArtifact(f.artifact, f.trust, f), /authenticity/);
});

test('authenticated wrong version, target, scope and capabilities are rejected', t => {
  const f = fixture(t);
  const original = fs.readFileSync(path.join(f.artifact, 'manifest.json'));
  for (const mutate of [m => { m.version = '0.2.0'; }, m => { m.target = 'darwin-arm64'; },
    m => { m.file = '../escape'; }, m => { m.capabilities.cleanup = 'enabled'; }, m => { m.capabilities.service = 'enabled'; }, m => { m.size = -1; }]) {
    fs.writeFileSync(path.join(f.artifact, 'manifest.json'), original);
    resign(f, mutate);
    assert.throws(() => verifyArtifact(f.artifact, f.trust, f));
  }
});

test('unknown manifest fields and oversized metadata are rejected', t => {
  const f = fixture(t);
  resign(f, m => { m.command = 'untrusted command'; });
  assert.throws(() => verifyArtifact(f.artifact, f.trust, f), /fields/);
  fs.writeFileSync(path.join(f.artifact, 'manifest.json'), 'x'.repeat(16385));
  assert.throws(() => verifyArtifact(f.artifact, f.trust, f), /bounded/);
});

test('symlink artifacts cannot substitute verified paths', { skip: process.platform === 'win32' }, t => {
  const f = fixture(t);
  fs.unlinkSync(path.join(f.artifact, 'omg'));
  fs.symlinkSync(f.binary, path.join(f.artifact, 'omg'));
  assert.throws(() => verifyArtifact(f.artifact, f.trust, f), /redirection/);
});

test('unsupported libc/architecture and missing native dependency have useful errors', () => {
  assert.throws(() => hostTarget({ platform: 'linux', arch: 'x64', report: { getReport: () => ({ header: {} }) } }), /libc/);
  assert.throws(() => hostTarget({ platform: 'linux', arch: 'riscv64', report: { getReport: () => ({ header: { glibcVersionRuntime: '2.39' } }) } }), /Unsupported platform/);
  assert.equal(hostTarget({ platform: 'darwin', arch: 'arm64' }), 'darwin-arm64');
  assert.throws(() => resolveArtifact(__dirname, 'linux-x64-gnu', '0.1.0'), /Missing native package/);
});

test('unprovisioned source wrapper never executes a binary', t => {
  const f = fixture(t);
  assert.throws(() => verifyArtifact(f.artifact, { schema: 1, keys: {} }, f), /No release signing key/);
});

test('offline npm install with scripts disabled runs verified native binary and uninstalls', {
  skip: process.platform !== 'linux' || !fs.existsSync(path.resolve(__dirname, '../../../target/debug/omg')),
  timeout: 60000,
}, t => {
  const f = fixture(t);
  const nativeBinary = path.resolve(__dirname, '../../../target/debug/omg');
  const nativeOut = path.join(f.directory, 'real-release');
  assemble({ binary: nativeBinary, target: hostTarget(), version: f.version,
    publicKey: f.publicFile, privateKey: f.privateFile, out: nativeOut });
  const pkg = path.join(nativeOut, 'offline-oh-my-gigabytes');
  const prefix = path.join(f.directory, 'prefix with spaces');
  const invoke = (command, args, options = {}) => {
    const result = spawnSync(command, args, { encoding: 'utf8', ...options });
    assert.equal(result.status, 0, result.stderr || result.error?.message);
    return result;
  };
  // Actual packed artifact, not a development symlink install.
  const packed = invoke('npm', ['pack', '--ignore-scripts', '--json', '--offline'], { cwd: pkg });
  const filename = JSON.parse(packed.stdout)[0].filename;
  const tarball = path.join(pkg, filename);
  invoke('npm', ['install', '--global', '--prefix', prefix, '--ignore-scripts', '--offline', '--no-audit', '--no-fund', tarball]);
  const cli = path.join(prefix, 'bin/omg');
  const version = invoke(process.execPath, [cli, '--version']);
  assert.match(version.stdout, /omg 0\.1\.0/);
  const state = path.join(f.directory, 'state');
  assert.match(invoke(process.execPath, [cli, '--state-dir', state, 'doctor']).stdout, /permanent delete: development-unqualified/);
  assert.ok(!fs.existsSync(state));
  const root = path.join(f.directory, 'scan-root');
  fs.mkdirSync(root);
  invoke(process.execPath, [cli, '--state-dir', state, 'scan', '--root', root]);
  const invalid = spawnSync(process.execPath, [cli, 'cleanup', '--yes'], { encoding: 'utf8' });
  assert.notEqual(invalid.status, 0);
  // Reinstallation is a package upgrade-path smoke, not a schema-version migration claim.
  invoke('npm', ['install', '--global', '--prefix', prefix, '--ignore-scripts', '--offline', '--no-audit', '--no-fund', tarball]);
  invoke('npm', ['uninstall', '--global', '--prefix', prefix, '--ignore-scripts', '--offline', 'oh-my-gigabytes']);
  assert.ok(!fs.existsSync(cli));
  assert.ok(fs.existsSync(path.join(state, 'index.sqlite')));
});
