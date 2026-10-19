'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const { assemble } = require('../../../scripts/assemble-release');
const { signRelease } = require('../../../scripts/sign-release');
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
  // The fixture binary is not executable, so the doctor cross-check is skipped for it. The shared
  // fixture is a full build so the tests below can exercise the delete capability claim.
  assemble({ binary, target, version, publicKey: publicFile, privateKey: privateFile, out, variant: 'full', checkBinary: false });
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

test('a release assembled without naming a variant is analysis-only', t => {
  const f = fixture(t);
  const out = path.join(f.directory, 'release-default');
  assemble({ binary: f.binary, target: f.target, version: f.version, publicKey: f.publicFile, privateKey: f.privateFile, out, checkBinary: false });
  const artifact = path.join(out, `oh-my-gigabytes-${f.target}`, 'artifact');
  const trust = JSON.parse(fs.readFileSync(path.join(out, 'oh-my-gigabytes/trust.json'), 'utf8'));
  assert.equal(verifyArtifact(artifact, trust, { target: f.target, version: f.version }).manifest.capabilities.cleanup, 'disabled');
});

test('a binary for another architecture is not executed during assembly', t => {
  if (process.platform !== 'linux' || process.arch !== 'x64') t.skip('needs an x86_64 Linux host');
  const f = fixture(t);
  const elf = Buffer.alloc(64);
  Buffer.from('7f454c46', 'hex').copy(elf);
  elf[4] = 2; elf[5] = 1; elf.writeUInt16LE(183, 18); // aarch64
  const binary = path.join(f.directory, 'fixture-arm64');
  fs.writeFileSync(binary, elf, { mode: 0o755 });
  const out = path.join(f.directory, 'release-arm64');
  // checkBinary stays on: running this file would fail, so success means it was not run.
  assemble({ binary, target: 'linux-arm64-gnu', version: f.version, publicKey: f.publicFile, privateKey: f.privateFile, out });
  assert.ok(fs.existsSync(path.join(out, 'oh-my-gigabytes-linux-arm64-gnu', 'artifact', 'manifest.json')));
});

test('a macOS release is analysis only and its analysis claim is checked', t => {
  const f = fixture(t);
  const macho = Buffer.alloc(64);
  macho.writeUInt32LE(0xfeedfacf, 0);
  macho.writeUInt32LE(0x0100000c, 4);
  const binary = path.join(f.directory, 'fixture-macho');
  fs.writeFileSync(binary, macho, { mode: 0o755 });
  const out = path.join(f.directory, 'release-macos');
  assemble({ binary, target: 'darwin-arm64', version: f.version, publicKey: f.publicFile, privateKey: f.privateFile, out, checkBinary: false });
  const artifact = path.join(out, 'oh-my-gigabytes-darwin-arm64', 'artifact');
  const trust = JSON.parse(fs.readFileSync(path.join(out, 'oh-my-gigabytes/trust.json'), 'utf8'));
  const expected = { target: 'darwin-arm64', version: f.version };
  const { manifest } = verifyArtifact(artifact, trust, expected);
  assert.deepEqual(manifest.capabilities, { analysis: 'development-macos', cleanup: 'disabled', service: 'disabled' });
  // A signed manifest that claims more than macOS offers is rejected.
  for (const change of [m => { m.capabilities.cleanup = 'development-unqualified'; },
    m => { m.capabilities.service = 'development-linux'; }, m => { m.capabilities.analysis = 'development-linux'; }]) {
    resign({ artifact, privateKey: f.privateKey }, change);
    assert.throws(() => verifyArtifact(artifact, trust, expected), /Unsupported advertised capabilities/);
    resign({ artifact, privateKey: f.privateKey }, m => { m.capabilities = { analysis: 'development-macos', cleanup: 'disabled', service: 'disabled' }; });
  }
});

// Minimal headers that pass the per-target format check, one for each release target.
function headers() {
  const elf = machine => { const b = Buffer.alloc(64); Buffer.from('7f454c46', 'hex').copy(b); b[4] = 2; b[5] = 1; b.writeUInt16LE(machine, 18); return b; };
  const pe = Buffer.alloc(128); pe.write('MZ'); pe.writeUInt32LE(64, 60); Buffer.from('50450000', 'hex').copy(pe, 64); pe.writeUInt16LE(0x8664, 68);
  const macho = cpu => { const b = Buffer.alloc(64); b.writeUInt32LE(0xfeedfacf, 0); b.writeUInt32LE(cpu, 4); return b; };
  return {
    'linux-x64-gnu': ['omg', elf(62)], 'linux-arm64-gnu': ['omg', elf(183)], 'win32-x64': ['omg.exe', pe],
    'darwin-arm64': ['omg', macho(0x0100000c)], 'darwin-x64': ['omg', macho(0x01000007)],
  };
}

function ciArtifacts(directory) {
  const artifacts = path.join(directory, 'ci-artifacts');
  for (const [target, [file, bytes]] of Object.entries(headers())) {
    const folder = path.join(artifacts, `omg-${target}`);
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, file), bytes);
    fs.writeFileSync(path.join(folder, 'SHA256SUMS'), `${crypto.createHash('sha256').update(bytes).digest('hex')}  ${file}\n`);
  }
  return artifacts;
}

const tarText = (tarball, member) => {
  const run = spawnSync('tar', ['-xOzf', tarball, member], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
  return run.stdout;
};

test('sign-release signs every target and names them all in the published wrapper', t => {
  const f = fixture(t);
  const artifacts = ciArtifacts(f.directory);
  const out = path.join(f.directory, 'signed');
  const result = signRelease({ artifacts, version: '0.1.0-beta.1', publicKey: f.publicFile, privateKey: f.privateFile, out, skipAttestation: true, checkBinary: false });
  assert.equal(result.targets.length, 5);
  const wrapper = JSON.parse(tarText(path.join(out, 'tarballs/oh-my-gigabytes-0.1.0-beta.1.tgz'), 'package/package.json'));
  assert.deepEqual(Object.keys(wrapper.optionalDependencies).sort(), Object.keys(headers()).map(target => `oh-my-gigabytes-${target}`).sort());
  assert.ok(Object.values(wrapper.optionalDependencies).every(version => version === '0.1.0-beta.1'));
  assert.equal(wrapper.private, true, 'npm refuses to publish unless the release is made publishable');
  assert.equal(fs.readFileSync(path.join(out, 'KEY-FINGERPRINT.txt'), 'utf8').trim(), result.keyId);
  // Every listed digest matches its file, and every tarball is listed.
  const lines = fs.readFileSync(path.join(out, 'SHA256SUMS'), 'utf8').trim().split('\n');
  assert.equal(lines.length, 11);
  for (const line of lines) {
    const [digest, name] = line.split('  ');
    const file = fs.existsSync(path.join(out, 'tarballs', name)) ? path.join(out, 'tarballs', name) : path.join(out, 'offline', name);
    assert.equal(crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'), digest, name);
  }
  assert.ok(!fs.existsSync(path.join(out, 'work')), 'staging folders are removed');
});

test('sign-release accepts relative paths', t => {
  const f = fixture(t);
  const relative = item => path.relative(process.cwd(), item);
  const out = path.join(f.directory, 'signed-relative');
  signRelease({ artifacts: relative(ciArtifacts(f.directory)), version: '0.1.0-beta.1', publicKey: relative(f.publicFile), privateKey: relative(f.privateFile),
    out: relative(out), skipAttestation: true, checkBinary: false });
  assert.ok(fs.existsSync(path.join(out, 'tarballs', 'oh-my-gigabytes-0.1.0-beta.1.tgz')));
});

test('sign-release can mark the packages publishable only when asked', t => {
  const f = fixture(t);
  const out = path.join(f.directory, 'signed-public');
  signRelease({ artifacts: ciArtifacts(f.directory), version: '0.1.0-beta.1', publicKey: f.publicFile, privateKey: f.privateFile, out, skipAttestation: true, publishable: true, checkBinary: false });
  assert.equal(JSON.parse(tarText(path.join(out, 'tarballs/oh-my-gigabytes-0.1.0-beta.1.tgz'), 'package/package.json')).private, false);
  assert.equal(JSON.parse(tarText(path.join(out, 'tarballs/oh-my-gigabytes-linux-x64-gnu-0.1.0-beta.1.tgz'), 'package/package.json')).private, false);
});

test('sign-release refuses a binary that does not match its recorded digest, a missing repo and an existing output', t => {
  const f = fixture(t);
  const artifacts = ciArtifacts(f.directory);
  const options = { artifacts, version: '0.1.0-beta.1', publicKey: f.publicFile, privateKey: f.privateFile, checkBinary: false };
  assert.throws(() => signRelease({ ...options, out: path.join(f.directory, 'no-repo') }), /--repo owner\/name is required/);
  fs.appendFileSync(path.join(artifacts, 'omg-linux-x64-gnu', 'omg'), 'x');
  assert.throws(() => signRelease({ ...options, out: path.join(f.directory, 'tampered'), skipAttestation: true }), /does not match its SHA256SUMS/);
  assert.throws(() => signRelease({ ...options, out: f.directory, skipAttestation: true }), /already exists/);
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
    target.startsWith('linux-') ? 'development-linux' : target.startsWith('win32-') ? 'development-windows' : target.startsWith('darwin-') ? 'development-macos' : 'disabled');
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
  // target/debug/omg is the default (full) build, so it must be signed as the full variant.
  assemble({ binary: nativeBinary, target: hostTarget(), version: f.version,
    publicKey: f.publicFile, privateKey: f.privateFile, out: nativeOut, variant: 'full' });
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
