// Chrome refuses to load an extension whose scripts contain Unicode noncharacters (U+FDD0-U+FDEF, U+xFFFE,
// U+xFFFF) or other text base::IsStringUTF8 rejects, with "It isn't UTF-8 encoded" (1.2 and 1.2.1 shipped a
// literal U+FFFF in a regular expression in citations-common.js). Write such characters as \u escapes.
const { test } = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..', '..');
function files(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    return e.isDirectory() ? files(p) : /\.(js|json|html|css)$/.test(e.name) ? [p] : [];
  });
}

test('extension and add-in files are text Chrome accepts', () => {
  const bad = [];
  for (const f of [...files(path.join(root, 'extension')), ...files(path.join(root, 'apple', 'citations'))]) {
    const s = fs.readFileSync(f, 'utf8');
    for (const ch of s) {
      const c = ch.codePointAt(0);
      if ((c >= 0xfdd0 && c <= 0xfdef) || (c & 0xfffe) === 0xfffe || c === 0xfffd) {
        bad.push(`${path.relative(root, f)}: U+${c.toString(16).toUpperCase()}`);
      }
    }
  }
  assert.deepStrictEqual(bad, []);
});
