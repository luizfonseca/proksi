// Writes `<file>.br` and `<file>.gz` next to each text file of dist/, embedded by build.rs
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';

const compressible = /\.(html|js|css|svg|json|txt)$/;

function walk(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      walk(path);
    } else if (compressible.test(name)) {
      const body = readFileSync(path);
      writeFileSync(`${path}.br`, brotliCompressSync(body, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }));
      writeFileSync(`${path}.gz`, gzipSync(body, { level: 9 }));
    }
  }
}

walk('dist');
