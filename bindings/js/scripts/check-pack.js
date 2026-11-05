// Packs the npm packages and checks each one against an explicit list of the
// files it must hold, and nothing else:
//
// - `cleanrules`: the entries and their declarations, the napi-rs loader
//   (`binding.cjs`), the modules of `lib/`, the WebAssembly module and its
//   glue (`wasm/`), README, LICENSE and NOTICE. No tests, Rust sources,
//   scripts, addons (`.node`), source maps or stray files. Every target of
//   `exports`, `main` and `types` is in it, and its optional dependencies are
//   the platform packages at its own version.
// - `@cleanrules/node-<platform>` (npm/<platform>): exactly its addon
//   (`cleanrules.<platform>.node`), README, LICENSE and NOTICE; the addon is a
//   binary of that system and processor, and a glibc one asks for glibc 2.28
//   at most (the versions it needs, read from its ELF version section).
//
// Every package has the main package's version. Before packing, LICENSE and
// NOTICE are copied into each platform package (they are in its `files`); its
// addon must already be there (CI: `napi artifacts`; by hand, copy
// `cleanrules.<platform>.node` into npm/<platform>). The main package's
// `prepack` refuses to pack without a current `wasm/` (`npm run build:wasm`).
//
//   node scripts/check-pack.js                          npm pack --dry-run of every package
//   node scripts/check-pack.js --out <dir>              pack into <dir>, then check the tarballs themselves
//   node scripts/check-pack.js --platforms darwin-arm64 only these platform packages (comma-separated; `none`)
//
// CI (node-binaries.yml) packs with `--out` and installs the tarballs with
// scripts/install-packed.js.

import fs from 'node:fs'
import path from 'node:path'
import { parseArgs } from 'node:util'

import { binaryKind, neededVersions, untar } from './inspect.js'
import { npm } from './npm.js'

const js = path.join(import.meta.dirname, '..')

/** The highest glibc a Linux glibc addon may ask for (manylinux_2_28, like Node 22). */
const MAX_GLIBC = [2, 28]

/** What the main package holds besides the modules of `lib/`. */
const MAIN_FILES = [
  'package.json',
  'README.md',
  'LICENSE',
  'NOTICE',
  'index.js',
  'index.wasm.js',
  'index.deno.js',
  'index.d.ts',
  'native.js',
  'native.wasm.js',
  'native.deno.js',
  'native.d.ts',
  'wasm.js',
  'wasm.d.ts',
  'binding.cjs',
  'binding.d.ts',
  'wasm/cleanrules.js',
  'wasm/cleanrules_bg.wasm',
]

const { values: options } = parseArgs({
  options: {
    out: { type: 'string' },
    platforms: { type: 'string' },
  },
})

const problems = []
const problem = (pkg, text) => problems.push(`${pkg}: ${text}`)

/** `npm pack --json`: the report of the one package packed (a lifecycle script may print before it). */
function pack(dir) {
  const args = ['pack', '--json']
  if (options.out === undefined) args.push('--dry-run')
  else args.push('--pack-destination', path.resolve(options.out))
  const stdout = npm(args, dir)
  const start = stdout.search(/^\[/m)
  if (start < 0) throw new Error(`npm pack printed no JSON report in ${dir}`)
  const [report] = JSON.parse(stdout.slice(start))
  return report
}

/** What a platform name (`linux-x64-gnu`) means for its package and addon. */
function expectations(platform) {
  const [os, cpu, abi] = platform.split('-')
  const format = { darwin: 'Mach-O', linux: 'ELF', win32: 'PE' }[os]
  const libc = { gnu: 'glibc', musl: 'musl' }[abi]
  return { os, cpu, libc, format }
}

/** Every file under `dir` (relative, with `/`). */
function walk(dir, base = dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name)
    return entry.isDirectory() ? walk(full, base) : [path.relative(base, full).split(path.sep).join('/')]
  })
}

/**
 * Packs the package in `dir` and gives what it holds: its manifest and a
 * reader of each file, from the tarball (`--out`) or from the directory
 * (`--dry-run`, which reads the files npm would pack).
 */
function packed(dir, name) {
  const report = pack(dir)
  if (options.out === undefined) {
    const names = report.files.map((file) => file.path)
    const read = (file) => (names.includes(file) ? fs.readFileSync(path.join(dir, file)) : undefined)
    return { report, names, read, manifest: JSON.parse(read('package.json')) }
  }
  const tarball = path.join(path.resolve(options.out), report.filename)
  const { files: entries, others } = untar(fs.readFileSync(tarball))
  for (const other of others) problem(name, `${other} is not a regular file`)
  const names = []
  for (const entry of entries.keys()) {
    if (entry.startsWith('package/')) names.push(entry.slice('package/'.length))
    else problem(name, `${entry} is outside package/`)
  }
  const reported = report.files.map((file) => file.path).sort()
  if (JSON.stringify(reported) !== JSON.stringify([...names].sort())) {
    problem(name, `the tarball holds ${names.length} files, but npm pack reported ${reported.length}`)
  }
  const read = (file) => entries.get(`package/${file}`)
  return { report, names, read, manifest: JSON.parse(read('package.json').toString('utf8')), tarball }
}

/** Reports what `names` has beyond `allowed` and what it lacks of `required`. */
function compare(pkg, names, required, allowed = required) {
  const have = new Set(names)
  for (const file of names) if (!allowed.has(file)) problem(pkg, `${file} must not be in the package`)
  for (const file of required) if (!have.has(file)) problem(pkg, `${file} is missing`)
}

const main = JSON.parse(fs.readFileSync(path.join(js, 'package.json'), 'utf8'))
const all = fs.readdirSync(path.join(js, 'npm')).sort()
const platforms =
  options.platforms === undefined
    ? all
    : options.platforms === 'none'
      ? []
      : options.platforms.split(',').map((platform) => platform.trim())
for (const platform of platforms) {
  if (!all.includes(platform)) {
    console.error(`no platform package ${platform}; there are ${all.join(', ')}`)
    process.exit(1)
  }
}
if (options.out !== undefined) fs.mkdirSync(options.out, { recursive: true })

const results = []

// The main package.
{
  const name = main.name
  const { names, read, manifest, tarball } = packed(js, name)
  const modules = walk(path.join(js, 'lib')).map((file) => `lib/${file}`)
  for (const file of modules) if (!file.endsWith('.js')) problem(name, `lib/ holds ${file}, which is not a module`)
  const required = new Set([...MAIN_FILES, ...modules.filter((file) => file.endsWith('.js'))])
  compare(name, names, required)
  const have = new Set(names)
  const targets = []
  const collect = (value) => {
    if (typeof value === 'string') targets.push(value)
    else if (value !== null && typeof value === 'object') Object.values(value).forEach(collect)
  }
  collect(manifest.exports)
  targets.push(manifest.main, manifest.types)
  for (const target of targets) {
    const file = String(target).replace(/^\.\//, '')
    if (!have.has(file)) problem(name, `${target}, named by package.json, is not in the package`)
  }
  const wasm = read('wasm/cleanrules_bg.wasm')
  if (wasm !== undefined && !wasm.subarray(0, 4).equals(Buffer.from([0, 0x61, 0x73, 0x6d]))) {
    problem(name, 'wasm/cleanrules_bg.wasm is not a WebAssembly module')
  }
  if (manifest.version !== main.version) {
    problem(name, `version ${manifest.version}, but package.json has ${main.version}`)
  }
  const optional = manifest.optionalDependencies ?? {}
  const expected = Object.fromEntries(all.map((platform) => [`@cleanrules/node-${platform}`, main.version]))
  if (JSON.stringify(Object.entries(optional).sort()) !== JSON.stringify(Object.entries(expected).sort())) {
    problem(name, `optionalDependencies ${JSON.stringify(optional)}, but they must be ${JSON.stringify(expected)}`)
  }
  const where = tarball ? ` in ${path.basename(tarball)}` : ''
  results.push(`${name}@${manifest.version}: ${names.length} files${where}`)
}

// The platform packages.
for (const platform of platforms) {
  const dir = path.join(js, 'npm', platform)
  const name = `@cleanrules/node-${platform}`
  const addon = `cleanrules.${platform}.node`
  for (const file of ['LICENSE', 'NOTICE']) fs.copyFileSync(path.join(js, file), path.join(dir, file))
  const { names, read, manifest, tarball } = packed(dir, name)
  compare(name, names, new Set(['package.json', 'README.md', 'LICENSE', 'NOTICE', addon]))
  const want = expectations(platform)
  if (manifest.name !== name) problem(name, `package.json names it ${manifest.name}`)
  if (manifest.version !== main.version) problem(name, `version ${manifest.version}, but cleanrules is ${main.version}`)
  if (manifest.main !== addon) problem(name, `main is ${manifest.main}, not ${addon}`)
  const list = (value) => JSON.stringify(value ?? null)
  if (list(manifest.os) !== list([want.os])) problem(name, `os ${list(manifest.os)}, not ["${want.os}"]`)
  if (list(manifest.cpu) !== list([want.cpu])) problem(name, `cpu ${list(manifest.cpu)}, not ["${want.cpu}"]`)
  if (list(manifest.libc) !== list(want.libc && [want.libc])) {
    problem(name, `libc ${list(manifest.libc)}, not ${list(want.libc && [want.libc])}`)
  }
  const bytes = read(addon)
  let detail = ''
  if (bytes !== undefined) {
    const kind = binaryKind(bytes)
    if (kind?.format !== want.format || kind?.arch !== want.cpu) {
      const is = kind ? `${kind.format} for ${kind.arch}` : 'no known binary'
      problem(name, `${addon} is ${is}, not ${want.format} for ${want.cpu}`)
    } else if (want.format === 'ELF') {
      // libgcc_s names two of its own symbols GLIBC_2.0 (`__register_frame_info`,
      // on Alpine arm64 too), so only what the other libraries ask counts.
      const glibc = neededVersions(bytes)
        .filter(({ file }) => !file.startsWith('libgcc_s.so'))
        .map(({ version }) => /^GLIBC_(\d+)\.(\d+)(?:\.\d+)?$/.exec(version))
        .filter((match) => match !== null)
        .map((match) => [Number(match[1]), Number(match[2])])
      const highest = glibc.reduce((a, b) => (b[0] > a[0] || (b[0] === a[0] && b[1] > a[1]) ? b : a), [0, 0])
      if (want.libc === 'musl' && glibc.length > 0) problem(name, `${addon} asks for glibc symbols`)
      if (want.libc === 'glibc') {
        if (glibc.length === 0) problem(name, `${addon} asks for no glibc symbol versions`)
        if (highest[0] > MAX_GLIBC[0] || (highest[0] === MAX_GLIBC[0] && highest[1] > MAX_GLIBC[1])) {
          problem(name, `${addon} needs glibc ${highest.join('.')}, but the most it may need is ${MAX_GLIBC.join('.')}`)
        }
        detail = `, needs glibc ${highest.join('.')}`
      }
    }
  }
  const where = tarball ? ` in ${path.basename(tarball)}` : ''
  results.push(`${name}@${manifest.version}: ${names.length} files${where}${detail}`)
}

for (const line of results) console.log(line)
if (problems.length > 0) {
  for (const line of problems) console.error(line)
  process.exit(1)
}
const count =
  results.length === 1 ? 'The package holds what it must' : `The ${results.length} packages hold what they must`
console.log(`${count}, and nothing else`)
