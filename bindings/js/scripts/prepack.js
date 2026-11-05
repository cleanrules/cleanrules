// Run by `npm pack` and `npm publish` (`prepack`): the package ships the
// WebAssembly module, so it must be built, and not older than the Rust code
// and the JavaScript glue it was built from. Build it with `npm run build:wasm`.

import fs from 'node:fs'
import path from 'node:path'

const js = path.join(import.meta.dirname, '..')
const root = path.join(js, '..', '..')
const module = path.join(js, 'wasm', 'cleanrules_bg.wasm')
const glue = path.join(js, 'wasm', 'cleanrules.js')

if (!fs.existsSync(module) || !fs.existsSync(glue)) {
  console.error('wasm/ is missing: run `npm run build:wasm` before packing')
  process.exit(1)
}
const built = Math.min(fs.statSync(module).mtimeMs, fs.statSync(glue).mtimeMs)

/** The newest modification time among `paths` (files, or directories walked). */
function newest(paths) {
  let time = 0
  for (const entry of paths) {
    if (!fs.existsSync(entry)) continue
    const stat = fs.statSync(entry)
    if (stat.isDirectory()) {
      for (const name of fs.readdirSync(entry, { recursive: true })) {
        const file = path.join(entry, name)
        if (fs.statSync(file).isFile()) time = Math.max(time, fs.statSync(file).mtimeMs)
      }
    } else {
      time = Math.max(time, stat.mtimeMs)
    }
  }
  return time
}

const sources = [
  path.join(js, 'src'),
  path.join(js, 'Cargo.toml'),
  path.join(root, 'crates', 'core', 'src'),
  path.join(root, 'crates', 'core', 'Cargo.toml'),
  path.join(root, 'Cargo.toml'),
  path.join(root, 'Cargo.lock'),
]
if (newest(sources) > built) {
  console.error('wasm/ is older than the Rust sources: run `npm run build:wasm` before packing')
  process.exit(1)
}
console.log('wasm/ is built and current')
