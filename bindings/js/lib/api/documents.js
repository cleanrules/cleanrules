// Loading and dumping conditions, rules, rule sets and decision tables, as
// Python's `cleanrules._documents`:
//
//   const rules = await load('pricing.yaml')
//   const text = dumps(rules, { format: 'yaml' })
//   const vip = ruleFromText('RULE vip IF customer.years > 5 THEN discount = 0.15')
//
// The documents are the JSON format of `spec/json-format.md`, the contract
// between languages; YAML files hold the same documents, the text syntax
// (`spec/text.md`) the same model, and CSV holds decision tables
// (`spec/tables.md`). The core reads and writes every document, strictly and
// within limits; this module checks the arguments, picks the format of a file
// from its extension, and makes the objects of the API of its build.

import { ExpressionError, RuleDefinitionError } from '../errors.js'
import { fileSystem } from '../files.js'
import { options, sameBuild, show } from './shared.js'

/** The format of each file extension, as in Python. */
const EXTENSIONS = { '.json': 'json', '.yaml': 'yaml', '.yml': 'yaml', '.rules': 'text', '.csv': 'csv' }

const LOAD_OPTIONS = ['format', 'limits', 'actions']
const DUMP_OPTIONS = ['format', 'omitActions']

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
function documents(native, expressions, rules, tables, build) {
  const { Expr, nodeOf, wrap } = expressions
  const { RuleSet, Rule, checkActions, fromNativeRule, fromNativeRuleSet, ruleNative, ruleSetNative, withAction } = rules
  const { DecisionTable, fromNativeTable, tableNative } = tables

  /** `rule` bound to the action registry `actions`, as `RuleSet.bind` binds. */
  const bindRule = (rule, actions) => new RuleSet([rule]).bind(actions).rules[0]

  /**
   * Read a condition, rule, rule set or decision table document: an `Expr`,
   * a `Rule`, a `RuleSet` or a `DecisionTable`. `format` is `'json'` (the
   * default), `'yaml'`, `'text'` or `'csv'`; `limits` as the native layer
   * takes them; `actions`, a registry that a rule or rule set read is bound
   * to (`RuleSet.bind`); without it, named actions stay unbound.
   */
  function loads(text, loadOptions) {
    const { format, limits, actions } = options(loadOptions, LOAD_OPTIONS, "{ format: 'yaml' }")
    const document = native.Document.read(text, { format, limits })
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
   * Write a condition, rule, rule set or decision table, in its canonical
   * form: `format` is `'json'` (compact, the default), `'yaml'`, `'text'` or
   * `'csv'` (decision tables only). A rule whose action is code is refused
   * (`UnsupportedError`) unless `omitActions`: a document holds an action's
   * name, never code.
   */
  function dumps(value, dumpOptions) {
    const { format, omitActions: omit = null } = options(dumpOptions, DUMP_OPTIONS, "{ format: 'yaml' }")
    const omitActions = omit ?? false
    if (typeof omitActions !== 'boolean') {
      throw new TypeError(`omitActions is a boolean, not ${show(omitActions)}`)
    }
    if (value instanceof DecisionTable) {
      return tableNative(value).write(format)
    }
    if (value instanceof Expr) {
      if (value.kind === 'value') {
        throw new ExpressionError(`only a condition can be a document, not the value ${value}`, {
          hint: 'compare it first, e.g. F.x.gt(0)',
        })
      }
      return native.writeCondition(nodeOf(value), format)
    }
    if (value instanceof Rule) {
      checkActions([value], '/rule', omitActions)
      return native.writeRule(ruleNative(value), format)
    }
    if (value instanceof RuleSet) {
      checkActions(value.rules, '/ruleset/rules/{}', omitActions)
      return ruleSetNative(value).write(format)
    }
    sameBuild(value, build)
    throw new TypeError(`dumps takes a condition, a rule, a rule set or a decision table, not ${show(value)}`)
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

  return { dump, dumps, load, loads, ruleFromText }
}

export { documents }
