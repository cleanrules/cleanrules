// Checks that the versions that must agree do:
//
// - the npm package, its optional dependencies and its platform packages
//   (npm/<platform>) all have the Cargo workspace's version, which the native
//   addon reports;
// - the `wasm-bindgen` crate is pinned (`=x.y.z` in Cargo.toml), locked
//   (Cargo.lock) and installed in CI (`wasm-bindgen@x.y.z` in ci.yml) at one
//   version, as the CLI that writes the module's glue must match the crate.
//
// CI runs it: node bindings/js/scripts/check-versions.js

import fs from 'node:fs'
import path from 'node:path'

const js = path.join(import.meta.dirname, '..')
const root = path.join(js, '..', '..')
const read = (...parts) => fs.readFileSync(path.join(...parts), 'utf8')

const cargo = read(root, 'Cargo.toml')
const workspace = /^\[workspace\.package\][^[]*?^version = "([^"]+)"/m.exec(cargo)?.[1]
if (workspace === undefined) {
  console.error('no version in [workspace.package] of Cargo.toml')
  process.exit(1)
}
const found = []
const pkg = JSON.parse(read(js, 'package.json'))
found.push(['package.json', pkg.version, workspace])
for (const [name, version] of Object.entries(pkg.optionalDependencies ?? {})) {
  found.push([`optionalDependencies ${name}`, version, workspace])
}
for (const dir of fs.readdirSync(path.join(js, 'npm'))) {
  const stub = JSON.parse(read(js, 'npm', dir, 'package.json'))
  found.push([`npm/${dir}/package.json`, stub.version, workspace])
}

const pinned = /^wasm-bindgen = "=([^"]+)"/m.exec(read(js, 'Cargo.toml'))?.[1]
const locked = /\[\[package\]\]\nname = "wasm-bindgen"\nversion = "([^"]+)"/.exec(read(root, 'Cargo.lock'))?.[1]
const installed = /wasm-bindgen@([0-9][^\s"']*)/.exec(read(root, '.github', 'workflows', 'ci.yml'))?.[1]
found.push(['wasm-bindgen in Cargo.lock', locked, pinned])
found.push(['wasm-bindgen-cli in ci.yml', installed, pinned])

const wrong = found.filter(([, version, expected]) => version !== expected || version === undefined)
for (const [where, version, expected] of wrong) {
  console.error(`${where}: ${version}, but it must be ${expected}`)
}
if (pinned === undefined) {
  console.error('bindings/js/Cargo.toml does not pin wasm-bindgen with "=x.y.z"')
}
if (wrong.length > 0 || pinned === undefined) process.exit(1)
console.log(`${found.length} versions agree: package ${workspace}, wasm-bindgen ${pinned}`)
