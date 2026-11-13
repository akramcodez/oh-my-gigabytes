'use strict';

// Apple and Microsoft signatures change the binary's bytes, so they are applied to a copy before the
// Ed25519 manifest is made over the final bytes (scripts/sign-release.js).
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  if (result.error || result.status !== 0) {
    throw new Error(`${path.basename(command)} ${args[0] || ''} failed: ${result.error?.message || result.stderr || result.stdout}`.trim());
  }
  return result;
}

// Size of the certificate table (data directory 4 of a PE32+ optional header); 0 when unsigned.
function authenticodeSize(data) {
  if (data.length < 64 || data.subarray(0, 2).toString() !== 'MZ') throw new Error('Not a PE file');
  const optional = data.readUInt32LE(60) + 24;
  if (optional + 2 > data.length || data.readUInt16LE(optional) !== 0x20b) throw new Error('Not a PE32+ file');
  const entry = optional + 112 + 4 * 8;
  if (entry + 8 > data.length) return 0;
  const offset = data.readUInt32LE(entry);
  const size = data.readUInt32LE(entry + 4);
  if (size && (offset < entry || offset + size > data.length)) throw new Error('PE certificate table lies outside the file');
  return size;
}

// The signer is the owner's own program (signtool, jsign or a wrapper around a cloud signing service).
// It gets the file as its only argument and no shell; the result must carry a certificate table.
function signWindows(file, signer) {
  if (authenticodeSize(fs.readFileSync(file))) throw new Error(`${file} is already Authenticode signed; expected the unsigned CI build`);
  run(signer, [file]);
  if (!authenticodeSize(fs.readFileSync(file))) throw new Error(`${signer} finished but ${path.basename(file)} has no Authenticode signature`);
  return 'authenticode';
}

// Developer ID with the hardened runtime and a secure timestamp, then notarization. A bare Mach-O
// cannot be stapled, so Gatekeeper looks the ticket up online the first time a download is opened.
function signMacos(file, { identity, notaryProfile }) {
  if (process.platform !== 'darwin') throw new Error('Apple signing and notarization run only on a Mac');
  run('codesign', ['--force', '--options', 'runtime', '--timestamp', '--sign', identity, file]);
  run('codesign', ['--verify', '--strict', '--verbose=2', file]);
  const details = run('codesign', ['--display', '--verbose=2', file]).stderr;
  if (!/^Authority=Developer ID Application: /m.test(details)) throw new Error(`${identity} is not a Developer ID Application certificate`);
  if (!/flags=0x[0-9a-f]*\(runtime\)/.test(details)) throw new Error('The hardened runtime was not applied');
  if (!/^Timestamp=/m.test(details)) throw new Error('The signature has no secure timestamp');
  const scratch = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'omg-notary-'));
  try {
    const zip = path.join(scratch, 'omg.zip');
    run('ditto', ['-c', '-k', '--keepParent', file, zip]);
    const output = run('xcrun', ['notarytool', 'submit', zip, '--keychain-profile', notaryProfile, '--wait', '--output-format', 'json']).stdout;
    const result = JSON.parse(output);
    if (result.status !== 'Accepted') {
      throw new Error(`Notarization ${result.status || 'failed'}; see xcrun notarytool log ${result.id} --keychain-profile ${notaryProfile}`);
    }
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
  return 'developer-id-notarized';
}

function checkOptions({ appleIdentity, notaryProfile }) {
  if (Boolean(appleIdentity) !== Boolean(notaryProfile)) {
    throw new Error('--apple-identity and --notary-profile go together: a Developer ID binary that is not notarized is still blocked');
  }
}

// Returns the binary to assemble: the original when nothing applies, or a signed copy under `work`.
function platformSign({ binary, target, work, appleIdentity, notaryProfile, windowsSigner }) {
  const apple = target.startsWith('darwin-') && appleIdentity;
  const windows = target.startsWith('win32-') && windowsSigner;
  if (!apple && !windows) return { binary, signature: 'none' };
  const copy = path.join(work, 'platform', target, path.basename(binary));
  fs.mkdirSync(path.dirname(copy), { recursive: true });
  fs.copyFileSync(binary, copy);
  fs.chmodSync(copy, 0o755);
  const signature = apple ? signMacos(copy, { identity: appleIdentity, notaryProfile }) : signWindows(copy, path.resolve(windowsSigner));
  return { binary: copy, signature };
}

module.exports = { authenticodeSize, checkOptions, platformSign };
