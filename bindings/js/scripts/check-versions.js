'use strict'

// Checks that the npm package, its optional dependencies and its platform
// packages (npm/<platform>) all have the Cargo workspace's version, which the
// native addon reports. CI runs it: node bindings/js/scripts/check-versions.js

const fs = require('node:fs')
const path = require('node:path')

const js = path.join(__dirname, '..')
const cargo = fs.readFileSync(path.join(js, '..', '..', 'Cargo.toml'), 'utf8')
const workspace = /^\[workspace\.package\][^[]*?^version = "([^"]+)"/m.exec(cargo)?.[1]
if (workspace === undefined) {
  console.error('no version in [workspace.package] of Cargo.toml')
  process.exit(1)
}
const found = []
const pkg = JSON.parse(fs.readFileSync(path.join(js, 'package.json'), 'utf8'))
found.push(['package.json', pkg.version])
for (const [name, version] of Object.entries(pkg.optionalDependencies ?? {})) {
  found.push([`optionalDependencies ${name}`, version])
}
for (const dir of fs.readdirSync(path.join(js, 'npm'))) {
  const stub = JSON.parse(fs.readFileSync(path.join(js, 'npm', dir, 'package.json'), 'utf8'))
  found.push([`npm/${dir}/package.json`, stub.version])
}
const wrong = found.filter(([, version]) => version !== workspace)
for (const [where, version] of wrong) {
  console.error(`${where}: ${version}, but the Cargo workspace is ${workspace}`)
}
if (wrong.length > 0) process.exit(1)
console.log(`${found.length} versions, all ${workspace}`)
