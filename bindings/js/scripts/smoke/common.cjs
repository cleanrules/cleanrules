// Shared by the smoke scripts that scripts/install-packed.js copies into a
// project where it installed the packed `cleanrules` (the project's directory
// is the working directory). CommonJS, so that both module systems use it.

'use strict'

const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')

const project = process.cwd()
const platform = process.env.CLEANRULES_SMOKE_PLATFORM
assert.ok(platform, 'CLEANRULES_SMOKE_PLATFORM names the platform package expected to load')

/**
 * A path as the file system has it, comparable across spellings: on Windows,
 * without case and without the `\\?\` prefix (Node opens addons by their
 * namespaced path, which the shared objects of the report may keep).
 */
function canonical(file) {
  if (process.platform !== 'win32') return fs.realpathSync.native(file)
  let plain = file
  if (plain.startsWith('\\\\?\\UNC\\')) plain = `\\\\${plain.slice(8)}`
  else if (plain.startsWith('\\\\?\\')) plain = plain.slice(4)
  return fs.realpathSync.native(plain).toLowerCase()
}

/** The cleanrules addons this process loaded: from require's cache and from the process's shared objects. */
function loadedAddons() {
  const addon = (file) => /^cleanrules\..+\.node$/i.test(path.basename(file))
  const files = Object.keys(require.cache).filter(addon)
  process.report.excludeNetwork = true
  for (const file of process.report.getReport().sharedObjects ?? []) if (addon(file)) files.push(file)
  return [...new Set(files.map(canonical))]
}

/** The rule set every route evaluates, in the rule syntax. */
const RULES = 'RULE big\nIF order.total > 100\nTHEN discount = 0.1'

/** Evaluates the same rule through the idiomatic API and the native layer of one build. */
function evaluate(api, native, how) {
  const { F, Rule, RuleSet, decimal, version } = api
  const pkg = JSON.parse(fs.readFileSync(path.join(project, 'node_modules', 'cleanrules', 'package.json'), 'utf8'))
  assert.equal(version, pkg.version, `${how}: version`)
  const rules = new RuleSet([new Rule('big', F.order.total.gt(100), { discount: decimal('0.1') })])
  const decision = rules.evaluate({ order: { total: 150 } })
  assert.deepEqual(decision.fired, ['big'], `${how}: the idiomatic API`)
  assert.equal(String(decision.output.discount), '0.1', `${how}: the idiomatic API's output`)
  const read = native.RuleSet.read(RULES, { format: 'text' })
  const output = read.evaluateJson('{"order": {"total": 150}}').outputJson()
  assert.equal(output, '{"discount":{"$decimal":"0.1"}}', `${how}: the native layer`)
  assert.equal(read.evaluateJson('{"order": {"total": 50}}').outputJson(), null, `${how}: the native layer, no rule`)
}

/** The addon that loaded is the platform package's, installed in the project; no other one loaded. */
function checkAddon(how) {
  const expected = path.join(project, 'node_modules', '@cleanrules', `node-${platform}`, `cleanrules.${platform}.node`)
  assert.ok(fs.existsSync(expected), `${how}: ${expected} is installed`)
  assert.deepEqual(loadedAddons(), [canonical(expected)], `${how}: the addon loaded from the platform package`)
  const main = path.join(project, 'node_modules', 'cleanrules')
  const stray = fs.readdirSync(main, { recursive: true }).filter((file) => String(file).endsWith('.node'))
  assert.deepEqual(stray, [], `${how}: the cleanrules package holds no addon`)
}

/** No addon loaded (the WebAssembly build). */
function checkNoAddon(how) {
  assert.deepEqual(loadedAddons(), [], `${how}: no addon loaded`)
}

/** `resolved` (a path or a file: URL) is `file` of the installed `cleanrules`. */
function checkResolved(resolved, file, how) {
  const full = resolved.startsWith('file:') ? require('node:url').fileURLToPath(resolved) : resolved
  assert.equal(canonical(full), canonical(path.join(project, 'node_modules', 'cleanrules', file)), how)
}

module.exports = { checkAddon, checkNoAddon, checkResolved, evaluate, platform }
