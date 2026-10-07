// SessionStart: print ▶ NEXT + latest shiplog entry, bump the staleness-sweep counter.
// Fail-open: any error prints what it can and exits 0.
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const SWEEP_THRESHOLD = 5;

const read = (rel) => {
  try {
    return readFileSync(join(root, rel), 'utf8');
  } catch {
    return '';
  }
};

// TODO and SHIPLOG live in private/ (a separate private repo); older clones had them at the root.
const privateOr = (name) => read(join('private', name)) || read(name);

const next = privateOr('TODO.md').split(/\r?\n/).find((l) => l.includes('▶ NEXT'));
console.log(next ? next.replace(/^>\s*/, '') : '▶ NEXT: (not found: is the private repo cloned into private/?)');

const latest = privateOr('SHIPLOG.md')
  .split(/^(?=## )/m)
  .find((s) => s.startsWith('## '));
if (latest) console.log('\nLatest ship:\n' + latest.trim());

try {
  const counter = join(root, '.claude', '.sweep_count');
  let n = 0;
  try {
    n = parseInt(readFileSync(counter, 'utf8'), 10) || 0;
  } catch {}
  n += 1;
  writeFileSync(counter, String(n));
  if (n >= SWEEP_THRESHOLD) {
    console.log(`\nStaleness sweep due (${n} opens since last sweep). Say "staleness sweep".`);
  }
} catch {}
