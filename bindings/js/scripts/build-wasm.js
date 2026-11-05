// Builds the WebAssembly module of the package into `wasm/`:
//
//   cargo build -p cleanrules-js --target wasm32-unknown-unknown --profile wasm
//   wasm-bindgen --target web --weak-refs ... --out-dir wasm --out-name cleanrules
//
// It needs the `wasm32-unknown-unknown` Rust target and the `wasm-bindgen`
// CLI at the version of the `wasm-bindgen` crate in Cargo.lock, which it
// checks first. `--target web` writes ES module glue that loads the module
// from a URL (`new URL('...', import.meta.url)`), which browsers and Deno
// fetch and `lib/wasm.js` reads from disk in Node. `--weak-refs` frees the
// Rust side of an object when JavaScript collects it.
//
//   node scripts/build-wasm.js     (npm run build:wasm)

import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import zlib from 'node:zlib'

const js = path.join(import.meta.dirname, '..')
const root = path.join(js, '..', '..')
const run = (command, args, options = {}) =>
  execFileSync(command, args, { cwd: js, stdio: ['ignore', 'pipe', 'inherit'], encoding: 'utf8', ...options })

const locked = /\[\[package\]\]\nname = "wasm-bindgen"\nversion = "([^"]+)"/.exec(
  fs.readFileSync(path.join(root, 'Cargo.lock'), 'utf8'),
)?.[1]
let cli
try {
  cli = run('wasm-bindgen', ['--version']).trim().split(/\s+/)[1]
} catch {
  console.error(`wasm-bindgen is not installed: cargo install --locked wasm-bindgen-cli --version ${locked}`)
  process.exit(1)
}
if (cli !== locked) {
  console.error(`wasm-bindgen ${cli} is installed, but Cargo.lock has the crate at ${locked}; they must match`)
  process.exit(1)
}

run('cargo', ['build', '-p', 'cleanrules-js', '--target', 'wasm32-unknown-unknown', '--profile', 'wasm'], {
  stdio: 'inherit',
})
const target = JSON.parse(run('cargo', ['metadata', '--format-version', '1', '--no-deps'])).target_directory
const built = path.join(target, 'wasm32-unknown-unknown', 'wasm', 'cleanrules_js.wasm')
const out = path.join(js, 'wasm')
fs.rmSync(out, { recursive: true, force: true })
run('wasm-bindgen', [
  '--target',
  'web',
  '--weak-refs',
  '--no-typescript',
  '--remove-name-section',
  '--remove-producers-section',
  '--out-dir',
  out,
  '--out-name',
  'cleanrules',
  built,
])
const module = fs.readFileSync(path.join(out, 'cleanrules_bg.wasm'))
const kib = (bytes) => `${(bytes / 1024).toFixed(0)} KiB`
console.log(`wasm/cleanrules_bg.wasm: ${kib(module.length)}, ${kib(zlib.gzipSync(module, { level: 9 }).length)} gzipped`)
