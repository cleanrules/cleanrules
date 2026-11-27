// Loading and dumping conditions, rules, rule sets, decision tables, bundles
// and case files, as Python's `cleanrules._documents`, and their canonical
// bytes and digests; dumping decisions too:
//
//   const rules = await load('pricing.yaml')
//   const text = dumps(rules, { format: 'yaml' })
//   const vip = ruleFromText('RULE vip IF customer.years > 5 THEN discount = 0.15')
//   const id = digest(rules)  // 'sha256:…'
//
// The documents are the JSON format of `spec/json-format.md`, the contract
// between languages; YAML files hold the same documents, the text syntax
// (`spec/text.md`) the same model, and CSV holds decision tables
// (`spec/tables.md`); JSON and YAML hold bundles (`spec/bundles.md`) and case
// files (`spec/tests.md`) too. The
// core reads and writes every document, strictly and
// within limits, and makes its canonical bytes and digest
// (`spec/json-format.md`, *Canonical bytes*, *Digests*); this module checks
// the arguments, picks the format of a file from its extension, and makes the
// objects of the API of its build.

import { ExpressionError, RuleDefinitionError, UnsupportedError } from '../errors.js'
import { fileSystem } from '../files.js'
import { Audit, TestRun } from './results.js'
import { options, sameBuild, show } from './shared.js'

/** The format of each file extension, as in Python. */
const EXTENSIONS = { '.json': 'json', '.yaml': 'yaml', '.yml': 'yaml', '.rules': 'text', '.csv': 'csv' }

const LOAD_OPTIONS = ['format', 'limits', 'actions']
const DUMP_OPTIONS = ['format', 'omitActions', 'rulesetDigest']
const DIGEST_OPTIONS = ['omitActions']

/** The file name of `path`, a string or a `file:` URL; a `TypeError` for anything else. */
function fileName(path, operation) {
  let text
  if (typeof path === 'string') {
    text = path
  } else if (typeof URL === 'function' && path instanceof URL) {
    try {
      text = decodeURIComponent(path.pathname)
    } catch {
      // A malformed escape: the name as written still has its extension.
      text = path.pathname
    }
  } else {
    throw new TypeError(`${operation} takes a path (a string or a file: URL), not ${show(path)}`)
  }
  return text.split(/[\\/]/).pop()
}

/** The format of the file `name`, from its extension. */
function formatOf(name) {
  const extension = /\.[^.]*$/.exec(name)?.[0].toLowerCase()
  const format = extension === undefined ? undefined : EXTENSIONS[extension]
  if (format === undefined) {
    throw new RangeError(
      `cannot tell the format of ${JSON.stringify(name)} from its extension (.json, .yaml, .yml, .rules or .csv); ` +
        "pass one, such as { format: 'json' }, 'yaml', 'text' or 'csv'",
    )
  }
  return format
}

/** The documents of one build (`build` names it). */
function documents(native, expressions, rules, tables, bundles, testCases, build) {
  const { Expr, nodeOf, wrap } = expressions
  const { Decision, RuleSet, Rule, checkActions, decisionNative, fromNativeRule, fromNativeRuleSet, ruleNative, ruleSetNative, withAction } =
    rules
  const { DecisionTable, fromNativeTable, tableNative } = tables
  const { Bundle, bundleNative, fromNativeBundle } = bundles
  const { Cases, casesNative, fromNativeCases } = testCases

  /** `rule` bound to the action registry `actions`, as `RuleSet.bind` binds. */
  const bindRule = (rule, actions) => new RuleSet([rule]).bind(actions).rules[0]

  /**
   * Read a condition, rule, rule set, decision table, bundle or case file
   * document: an `Expr`, a `Rule`, a `RuleSet`, a `DecisionTable`, a
   * `Bundle` or a `Cases`. `format`
   * is `'json'` (the default), `'yaml'`, `'text'` or `'csv'`; `limits` as the
   * native layer takes them; `actions`, a registry that a rule or rule set
   * read is bound to (`RuleSet.bind`); without it, named actions stay unbound.
   */
  function loads(text, loadOptions) {
    const { format, limits, actions } = options(loadOptions, LOAD_OPTIONS, "{ format: 'yaml' }")
    const document = native.Document.read(text, { format, limits })
    // A case file (spec/tests.md) keeps its document.
    if (document.kind === 'cases') {
      if (actions !== undefined && actions !== null) {
        throw new TypeError('a case file is not bound to actions: test cases never run them')
      }
      return fromNativeCases(document)
    }
    // A bundle keeps its document.
    if (document.kind === 'bundle') {
      if (actions !== undefined && actions !== null) {
        throw new TypeError(
          'a bundle is not bound to actions: bind its rules with bundle.ruleset.bind(actions)',
        )
      }
      return fromNativeBundle(document)
    }
    try {
      switch (document.kind) {
        case 'condition':
          return wrap(native.documentCondition(document))
        case 'rule': {
          const rule = fromNativeRule(native.documentRule(document))
          return actions === undefined || actions === null ? rule : bindRule(rule, actions)
        }
        case 'decision_table':
          return fromNativeTable(native.documentTable(document))
        default: {
          const ruleset = fromNativeRuleSet(native.RuleSet.fromDocument(document))
          return actions === undefined || actions === null ? ruleset : ruleset.bind(actions)
        }
      }
    } finally {
      document.free?.()
    }
  }

  /**
   * What a document of `value` holds, for `operation`: `[kind, native]`, with
   * the checks of `dumps` (a value node is not a document; a rule whose action
   * is code is refused unless `omitActions`).
   */
  function documentOf(value, omit, operation) {
    const omitActions = omit ?? false
    if (typeof omitActions !== 'boolean') {
      throw new TypeError(`omitActions is a boolean, not ${show(omitActions)}`)
    }
    if (value instanceof DecisionTable) return ['table', tableNative(value)]
    if (value instanceof Bundle) return ['bundle', bundleNative(value)]
    if (value instanceof Cases) return ['cases', casesNative(value)]
    if (value instanceof Expr) {
      if (value.kind === 'value') {
        throw new ExpressionError(`only a condition can be a document, not the value ${value}`, {
          hint: 'compare it first, e.g. F.x.gt(0)',
        })
      }
      return ['condition', nodeOf(value)]
    }
    if (value instanceof Rule) {
      checkActions([value], '/rule', omitActions)
      return ['rule', ruleNative(value)]
    }
    if (value instanceof RuleSet) {
      checkActions(value.rules, '/ruleset/rules/{}', omitActions)
      return ['ruleset', ruleSetNative(value)]
    }
    sameBuild(value, build)
    const what = operation === 'dumps' ? 'a decision table, a bundle, a case file or a decision' : 'a decision table, a bundle or a case file'
    throw new TypeError(`${operation} takes a condition, a rule, a rule set, ${what}, not ${show(value)}`)
  }

  /**
   * Write a condition, rule, rule set, decision table, bundle, case file
   * (`Cases`) or decision, in its canonical form: `format` is `'json'`
   * (compact, the default), `'yaml'`, `'text'` (conditions, rules and rule
   * sets) or `'csv'` (decision tables only). A rule whose action is code is refused
   * (`UnsupportedError`) unless `omitActions`: a document holds an action's
   * name, never code. With `rulesetDigest`, a decision carries the digest of
   * the rule set that made it (`spec/json-format.md`, *In decisions*); a
   * decision read with one keeps it either way.
   */
  function dumps(value, dumpOptions) {
    const { format, omitActions, rulesetDigest } = options(dumpOptions, DUMP_OPTIONS, "{ format: 'yaml' }")
    if (value instanceof Decision) return decisionNative(value).write(format, { rulesetDigest })
    if (value instanceof Audit) {
      throw new TypeError('an audit is output only, so dumps does not write it: use JSON.stringify(report) for its document, or String(report) for its text')
    }
    if (value instanceof TestRun) {
      throw new TypeError('a test run is output only, so dumps does not write it: use JSON.stringify(run) for its document, or String(run) for its text')
    }
    if (rulesetDigest !== undefined && rulesetDigest !== null) {
      throw new TypeError(`rulesetDigest is an option of decisions, not of ${show(value)}`)
    }
    const [kind, document] = documentOf(value, omitActions, 'dumps')
    switch (kind) {
      case 'condition':
        return native.writeCondition(document, format)
      case 'rule':
        return native.writeRule(document, format)
      default:
        return document.write(format)
    }
  }

  /**
   * The canonical bytes of the document of a condition, rule, rule set,
   * decision table, bundle or case file (`spec/json-format.md`, *Canonical
   * bytes*): the UTF-8 JSON
   * text of its canonical form, every object's members sorted by key, by code
   * point, without whitespace. The same checks as `dumps`.
   */
  function canonicalBytes(value, digestOptions) {
    const { omitActions } = options(digestOptions, DIGEST_OPTIONS, '{ omitActions: true }')
    const [kind, document] = documentOf(value, omitActions, 'canonicalBytes')
    switch (kind) {
      case 'condition':
        return native.canonicalCondition(document)
      case 'rule':
        return native.canonicalRule(document)
      default:
        return document.canonicalBytes()
    }
  }

  /**
   * The digest of the document of a condition, rule, rule set, decision
   * table, bundle or case file (`spec/json-format.md`, *Digests*): `sha256:` and the 64 lowercase
   * hexadecimal digits of the SHA-256 hash of its canonical bytes. The same
   * checks as `dumps`.
   */
  function digest(value, digestOptions) {
    const { omitActions } = options(digestOptions, DIGEST_OPTIONS, '{ omitActions: true }')
    const [kind, document] = documentOf(value, omitActions, 'digest')
    switch (kind) {
      case 'condition':
        return native.digestCondition(document)
      case 'rule':
        return native.digestRule(document)
      default:
        return document.digest()
    }
  }

  /**
   * Read one rule written in the text syntax (`spec/text.md`); without a
   * `RULE` line it is named `rule_1`. `do` is its action, a function (text
   * holds no code; a name the text gives with `DO` is bound to it), or
   * `actions` a registry, as `loads` takes; `limits` as `loads`.
   */
  function ruleFromText(text, textOptions) {
    const given = options(textOptions, ['do', 'actions', 'limits'], '{ do: notify }')
    const { do: action = null, actions = null, limits } = given
    if (action !== null && actions !== null) {
      throw new TypeError('ruleFromText takes do or actions, not both')
    }
    if (action !== null && typeof action !== 'function') {
      throw new RuleDefinitionError(`do must be a function, not ${show(action)}`, {
        hint: 'name an action in the text (DO notify) and pass actions: { notify }, or pass the function as do',
      })
    }
    const rule = fromNativeRule(native.ruleText(text, limits))
    if (actions !== null) return bindRule(rule, actions)
    return action === null ? rule : withAction(rule, action)
  }

  /**
   * Read a document from a file, as `loads` does: a promise. The format is
   * the extension's (`.json`, `.yaml`, `.yml`, `.rules`, `.csv`) unless
   * given; a file larger than the size limit is not read. In Node and Deno;
   * browsers have no files (`UnsupportedError`: use `loads`).
   */
  async function load(path, loadOptions) {
    const given = options(loadOptions, LOAD_OPTIONS, "{ format: 'yaml' }")
    const files = fileSystem('load')
    const name = fileName(path, 'load')
    const format = given.format ?? formatOf(name)
    native.fileSizeCheck(name, await files.size(path), format, given.limits)
    const bytes = await files.read(path)
    return loads(bytes, { ...given, format })
  }

  /**
   * Write a document to a file, as `dumps` does: a promise. The format is the
   * extension's unless given, and a rule text ends with a line break. In Node
   * and Deno; browsers have no files (`UnsupportedError`: use `dumps`).
   */
  async function dump(value, path, dumpOptions) {
    const given = options(dumpOptions, DUMP_OPTIONS, "{ format: 'yaml' }")
    const files = fileSystem('dump')
    const format = given.format ?? formatOf(fileName(path, 'dump'))
    const text = dumps(value, { ...given, format })
    await files.write(path, format === 'text' ? `${text}\n` : text)
  }

  return { canonicalBytes, digest, dump, dumps, load, loads, ruleFromText }
}

export { documents }
