// Installs packed tarballs (scripts/check-pack.js --out <dir>) into a new,
// empty project, as a user would install `cleanrules`, then runs the smoke
// scripts of scripts/smoke/ there:
//
// - `esm.mjs` and `cjs.cjs`: `cleanrules` and `cleanrules/native` by `import`
//   and by `require`, a rule evaluated through each, and the addon loaded
//   from the platform package in the project's node_modules (and from nowhere
//   else);
// - `browser.mjs`, under `node --conditions=browser`: both entries resolve to
//   the WebAssembly build, which evaluates the rule, and no addon is loaded.
//
// The platform packages are not on the registry, so the project points npm at
// the tarballs: `cleanrules` as a `file:` dependency, and each
// `@cleanrules/node-<platform>` it depends on through an `overrides` entry
// with a `file:` spec. npm still resolves the optional dependencies as for a
// registry install (it picks the one package whose `os`, `cpu` and `libc` fit
// and skips the others), and `--offline`, with a cache of its own, keeps it
// off the network: a dependency missing from the tarballs fails instead of
// being fetched (an optional one is skipped, which `--all` forbids).
//
//   node scripts/install-packed.js <tarballs> [--project <dir>] [--platform <platform>] [--all]
//
// <tarballs> holds `cleanrules-<version>.tgz` and the platform packages'
// tarballs; --project is a directory that does not exist or is empty (by
// default, a new temporary one); --platform is the platform package expected
// to load (by default, this machine's); --all requires a tarball for every
// platform package.

import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { parseArgs } from 'node:util'

import { untar } from './inspect.js'
import { npm } from './npm.js'

const js = path.join(import.meta.dirname, '..')

const { values: options, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    project: { type: 'string' },
    platform: { type: 'string' },
    all: { type: 'boolean', default: false },
  },
})
if (positionals.length !== 1) {
  console.error('usage: node scripts/install-packed.js <tarballs> [--project <dir>] [--platform <platform>] [--all]')
  process.exit(2)
}

/** This machine's platform package, as napi-rs names them. */
function hostPlatform() {
  const { platform, arch } = process
  if (platform === 'linux') {
    const glibc = process.report.getReport().header.glibcVersionRuntime
    return `linux-${arch}-${glibc ? 'gnu' : 'musl'}`
  }
  return platform === 'win32' ? `win32-${arch}-msvc` : `${platform}-${arch}`
}

const platform = options.platform ?? hostPlatform()
const platforms = fs.readdirSync(path.join(js, 'npm')).sort()
if (!platforms.includes(platform)) {
  console.error(`no platform package ${platform}; there are ${platforms.join(', ')}`)
  process.exit(1)
}

// The tarballs, by the package each one holds.
const source = path.resolve(positionals[0])
const tarballs = new Map()
for (const file of fs.readdirSync(source).filter((name) => name.endsWith('.tgz'))) {
  const manifest = untar(fs.readFileSync(path.join(source, file))).files.get('package/package.json')
  if (manifest === undefined) continue
  const { name, version } = JSON.parse(manifest.toString('utf8'))
  if (tarballs.has(name)) {
    console.error(`${source} holds two tarballs of ${name}: ${tarballs.get(name).file} and ${file}`)
    process.exit(1)
  }
  tarballs.set(name, { file, version })
}
const main = tarballs.get('cleanrules')
if (main === undefined) {
  console.error(`${source} holds no tarball of cleanrules`)
  process.exit(1)
}
const missing = platforms.filter((name) => !tarballs.has(`@cleanrules/node-${name}`))
if (missing.includes(platform) || (options.all && missing.length > 0)) {
  console.error(`${source} has no tarball of @cleanrules/node-${missing.join(', @cleanrules/node-')}`)
  process.exit(1)
}
for (const [name, { version }] of tarballs) {
  if (version !== main.version) {
    console.error(`${name} is version ${version}, but cleanrules is ${main.version}`)
    process.exit(1)
  }
}

// A new project, with the tarballs in it.
const project = options.project
  ? path.resolve(options.project)
  : fs.mkdtempSync(path.join(os.tmpdir(), 'cleanrules-smoke-'))
if (fs.existsSync(project) && fs.readdirSync(project).length > 0) {
  console.error(`${project} is not empty; give a new or empty directory`)
  process.exit(1)
}
fs.mkdirSync(path.join(project, 'packages'), { recursive: true })
for (const { file } of tarballs.values()) {
  fs.copyFileSync(path.join(source, file), path.join(project, 'packages', file))
}
const spec = (name) => `file:packages/${tarballs.get(name).file}`
const overrides = Object.fromEntries(
  [...tarballs.keys()].filter((name) => name.startsWith('@cleanrules/node-')).map((name) => [name, spec(name)]),
)
const manifest = {
  name: 'cleanrules-smoke',
  version: '0.0.0',
  private: true,
  dependencies: { cleanrules: spec('cleanrules') },
  overrides,
}
fs.writeFileSync(path.join(project, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`)
for (const file of fs.readdirSync(path.join(js, 'scripts', 'smoke'))) {
  fs.copyFileSync(path.join(js, 'scripts', 'smoke', file), path.join(project, file))
}

console.log(`installing cleanrules ${main.version} into ${project}`)
if (missing.length > 0) console.log(`(no tarball of ${missing.join(', ')}: npm skips them)`)
const cache = path.join(project, '.npm-cache')
npm(['install', '--offline', '--no-audit', '--no-fund', '--no-update-notifier', '--cache', cache], project)

// npm installed `cleanrules` and the one platform package for this machine,
// as copies (not links into the repository).
const modules = path.join(project, 'node_modules')
const scope = path.join(modules, '@cleanrules')
const installed = fs.existsSync(scope) ? fs.readdirSync(scope) : []
const problems = []
if (JSON.stringify(installed) !== JSON.stringify([`node-${platform}`])) {
  problems.push(`npm installed @cleanrules/{${installed.join(',')}}, not just node-${platform}`)
}
for (const dir of ['cleanrules', `@cleanrules/node-${platform}`]) {
  const full = path.join(modules, dir)
  if (!fs.existsSync(full) || fs.lstatSync(full).isSymbolicLink()) {
    problems.push(`node_modules/${dir} is not a directory`)
  }
}
if (problems.length > 0) {
  for (const line of problems) console.error(line)
  process.exit(1)
}

// The smoke scripts, from the project, with nothing that redirects the loader.
const env = { ...process.env, CLEANRULES_SMOKE_PLATFORM: platform }
delete env.NAPI_RS_NATIVE_LIBRARY_PATH
delete env.NODE_OPTIONS
delete env.CLEANRULES_BUILD
for (const args of [['esm.mjs'], ['cjs.cjs'], ['--conditions=browser', 'browser.mjs']]) {
  console.log(`node ${args.join(' ')}`)
  const result = spawnSync(process.execPath, args, { cwd: project, env, stdio: 'inherit' })
  if (result.status !== 0) {
    console.error(`node ${args.join(' ')} failed (exit ${result.status ?? result.signal})`)
    process.exit(1)
  }
}
console.log(`the packed cleanrules ${main.version} installs and loads, with @cleanrules/node-${platform}`)
