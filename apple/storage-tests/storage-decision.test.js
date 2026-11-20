// Where the library lives on Mac, iPhone and iPad (apple/Shared/StorageDecision.swift): what start-up
// does on a fresh device, a new device whose iCloud has the library, and for existing users; how a
// library is copied when the user moves it. Compiled with a Swift test driver; needs the Swift
// compiler (macOS with Xcode), skipped elsewhere.
// Run: node --test apple/storage-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const ROOT = path.join(__dirname, '..', '..');
const swiftc = process.platform === 'darwin' && spawnSync('xcrun', ['--find', 'swiftc'], { encoding: 'utf8' }).status === 0;

test('library storage: start-up decision and copy rules', { skip: !swiftc && 'no Swift compiler', timeout: 300000 }, () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'lunta-storage-'));
  const bin = path.join(dir, 'driver');
  const built = spawnSync('xcrun', ['swiftc', '-Onone', '-parse-as-library', '-o', bin,
    path.join(ROOT, 'apple', 'Shared', 'StorageDecision.swift'), path.join(__dirname, 'swift', 'StorageDecisionTests.swift')], { encoding: 'utf8' });
  assert.equal(built.status, 0, built.stderr);
  const run = spawnSync(bin, [], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stdout + run.stderr);
  assert.match(run.stdout, /all passed/);
});
