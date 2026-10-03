// Independent ECMAScript serialization check for the Rust lab-jcs-v1 vectors.
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';

const path = new URL('../contracts/v2/fixtures/jcs-vectors.json', import.meta.url);
const vectors = JSON.parse(readFileSync(path, 'utf8'));

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

for (const vector of vectors) {
  const bytes = canonical(JSON.parse(vector.input));
  const hash = createHash('sha256')
    .update('lab-jcs-v1\0test-vector\0', 'utf8')
    .update(bytes, 'utf8')
    .digest('hex');
  const digest = `lab-jcs-v1:test-vector:${hash}`;
  if (process.argv.includes('--update')) {
    vector.canonical = bytes;
    vector.digest = digest;
  } else if (vector.canonical !== bytes || vector.digest !== digest) {
    throw new Error(`JCS mismatch: ${vector.name}`);
  }
}
if (process.argv.includes('--update')) {
  writeFileSync(path, `${JSON.stringify(vectors, null, 2)}\n`);
}
console.log(`PASS: ${vectors.length} ECMAScript JCS vectors`);
