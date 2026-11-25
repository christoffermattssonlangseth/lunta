// The reader's Summary card while it is written (apple/Shared/SummaryWriter.swift): partial texts
// reach the page at most 4 times a second, and summaries aren't written without being asked in Low
// Power Mode or with the device hot. The page's side (no change while the page moves, only the
// finished summary out of sight) is in scripts/reader-tests/summary.test.mjs. Compiled with a Swift
// test driver; needs the Swift compiler and FoundationModels (macOS 26 SDK), skipped elsewhere.
// Run: node --test apple/summary-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const ROOT = path.join(__dirname, '..', '..');
const swiftc = process.platform === 'darwin' && spawnSync('xcrun', ['--find', 'swiftc'], { encoding: 'utf8' }).status === 0;

test('summary: throttled updates, no automatic summary in Low Power Mode or hot', { skip: !swiftc && 'no Swift compiler', timeout: 300000 }, () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'lunta-summary-'));
  const bin = path.join(dir, 'driver');
  const built = spawnSync('xcrun', ['swiftc', '-Onone', '-parse-as-library', '-o', bin,
    path.join(ROOT, 'apple', 'Shared', 'SummaryWriter.swift'), path.join(__dirname, 'swift', 'SummaryBudgetTests.swift')], { encoding: 'utf8' });
  assert.equal(built.status, 0, built.stderr);
  const run = spawnSync(bin, [], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stdout + run.stderr);
  assert.match(run.stdout, /all passed/);
});
