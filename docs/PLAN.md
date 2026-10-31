# Plan de desarrollo de RulePy

> Documento vivo. Es la fuente de verdad del roadmap: cada tarea es una casilla, y al completarla se marca y se anotan debajo las desviaciones. Claude lo sigue mediante `/plan-task`.
>
> Creado: 2026-10-30.

## 0. Visión

RulePy será **una sola librería con un único núcleo** que cubra:

| Caso de uso | Rival principal | Por dónde ganamos |
|---|---|---|
| Reglas de negocio | zen-engine, rule-engine, business-rules | Ida y vuelta código ↔ YAML/texto, tablas de decisión, **auditoría estática**, explicación con contrafactuales |
| Sistema experto | experta, durable_rules, clipspy | Mantenido, tipado, Rete (en Rust), mantenimiento de verdad, hechos con grado de certeza, árbol de derivación |
| Validación de datos | pandera, Great Expectations | Mismas reglas que en negocio, reglas entre registros, compilación a polars, explicación por fila |
| Explicaciones de ML | imodels, alibi (anchors) | Las reglas extraídas se **ejecutan y auditan**, extracción difusa (fuzzytool), guardarraíles compatibles con sklearn |
| Lógica difusa | simpful, scikit-fuzzy | fuzzytool integrada con inferencia simbólica y reglas mixtas nítidas y difusas |

Lo que nos diferencia es la combinación: **condiciones como AST** + **lógica intercambiable (nítida o difusa)** + **explicación y auditoría en todo** + **núcleo Rust opcional, verificado contra un oráculo Python**.

## 1. Decisiones de arquitectura

| # | Decisión | Motivo |
|---|---|---|
| D1 | Las condiciones son un AST inmutable, nunca lambdas | Un mismo árbol se serializa, explica, vectoriza, audita y compila a Rust |
| D2 | El evaluador es genérico sobre `Logic` (`BooleanLogic` de Kleene con 3 valores; `FuzzyLogic`) | Las reglas nítidas y difusas comparten motor, y los nulos se tratan igual que en polars |
| D3 | Dos paquetes: `rulepy` (Python puro, hatchling) y `rulepy-core` (Rust, maturin, opcional mediante `rulepy[fast]`) | Se instala en cualquier sitio sin compilador, y Rust no bloquea las primeras fases |
| D4 | La implementación Python es el **oráculo** semántico | Los tests diferenciales con hypothesis dan la robustez que no tienen los rivales |
| D5 | El formato del AST está especificado y versionado en `docs/spec/` (`rulepy_ast: n`) | Es el contrato Python ↔ Rust ↔ ficheros de usuario |
| D6 | El núcleo no tiene dependencias, y todo lo demás va en extras con importación perezosa | `pip install rulepy` es ligero |
| D7 | polars es el backend columnar principal; pandas se soporta por conversión o con el evaluador por filas | polars ya es Rust y usa nulos de Kleene: obtenemos velocidad sin escribir Rust |
| D8 | fuzzytool es el motor difuso; RulePy solo lo adapta | Se evita duplicar código y se mantiene coherencia con la otra librería del autor |
| D9 | Sin `eval`/`exec`: DSL con parser propio, `yaml.safe_load` y regex limitadas al subconjunto del crate `regex` | Seguridad y paridad con Rust |
| D10 | uv workspace (Python) + Cargo workspace (Rust) en un monorepo | Un único repositorio, un único CI y versiones sincronizadas |

### Decisiones pendientes (confirmar con el autor)

- [x] Python mínimo: **3.11**, aplicado en 1.2 (fuzzytool admite ≥3.9; 3.11 aporta `Self`, `StrEnum` y grupos de excepciones). Wheels Rust `abi3-py311` más una build *free-threaded* 3.14t.
- [x] Licencia: **MIT**, como fuzzytool (aplicada en 1.1).
- [ ] Organización de GitHub y dominio de la documentación (¿`rulepy.github.io`, siguiendo el patrón de fuzzytool?).
- [ ] Hacer en fuzzytool 0.8 el cambio de `Antecedent.__and__/__or__`, que debe devolver `NotImplemented` cuando recibe tipos ajenos (ver fase 5).

## 2. Estructura objetivo del repositorio

```
RulePy/
├── CLAUDE.md                  # instrucciones para Claude
├── .claude/                   # settings, hooks, agents, skills
├── pyproject.toml             # paquete rulepy + uv workspace + ruff/pyright/pytest
├── Cargo.toml                 # workspace Rust (fase 7)
├── src/rulepy/
│   ├── __init__.py            # API pública (__all__)
│   ├── errors.py
│   ├── _backend.py            # selección python/rust (RULEPY_BACKEND)
│   ├── core/                  # expr, logic, evaluate, rule, ruleset, trace, facts
│   ├── engines/               # sequential.py, forward/ (wm, agenda, naive, rete, tms), backward.py
│   ├── io/                    # serialize, yaml, text (DSL), tables (tablas de decisión)
│   ├── validation/            # rules, report, rowwise, polars_backend
│   ├── fuzzy/                 # logic, adapter (fuzzytool), infer
│   ├── audit/                 # intervals, z3_backend, report
│   └── ml/                    # extract, surrogate, metrics, guard
├── crates/
│   ├── rulepy-core/           # Rust puro: AST serde, bytecode VM, Arrow, Rete
│   └── rulepy-py/             # bindings PyO3 → módulo rulepy_core (maturin)
├── tests/                     # refleja src/; strategies.py, parity/, properties/, snapshots/
├── benchmarks/                # run.py, suites/, results/
├── docs/                      # PLAN.md, spec/, sitio mkdocs-material
└── examples/                  # scripts y notebooks por caso de uso
```

## 3. Fases

Cada fase termina en una versión publicable (0.x). Los tamaños son relativos (S/M/L/XL) y no estimaciones de tiempo.

---

### Fase 0. Especificaciones para Claude ✅ · tamaño S

- [x] 0.1 `CLAUDE.md`: visión, comandos, invariantes, convenciones, definición de terminado y qué no hacer.
- [x] 0.2 `.claude/settings.json`: permisos (uv, pytest, ruff, pyright, cargo y maturin permitidos; `uv add`, `cargo add`, push y tag piden confirmación; la publicación está denegada).
- [x] 0.3 `.claude/hooks/format.sh`: tras cada Write/Edit ordena imports y formatea `.py` (ruff vía uvx) y `.rs` (rustfmt). Probado.
- [x] 0.4 Subagentes: `api-reviewer` (solo lectura), `parity-tester` y `rust-engineer`.
- [x] 0.5 Skills: `/plan-task`, `/add-operator` y `/bench`.
- [x] 0.6 Este plan.

---

### Fase 1. Andamiaje del proyecto · v0.0.1 · tamaño S

**Objetivo**: un repositorio vacío pero con todas las puertas de calidad funcionando en local y en CI.

- [x] 1.1 `git init`, `.gitignore` (Python, Rust, `.claude/settings.local.json`), `LICENSE`, `README.md` mínimo y `CHANGELOG.md`.
  - Rama `main`, todavía sin commits. Licencia MIT (© 2026 Jose L. Salmeron). README y CHANGELOG en inglés (de cara al público); el README avisa de que la API es provisional.
- [x] 1.2 `pyproject.toml`: hatchling, estructura `src/`, `requires-python`, extras vacíos ya declarados (`fuzzy`, `polars`, `pandas`, `yaml`, `audit`, `ml`, `fast`, `bench`, `docs`, `dev`).
  - Versión `0.0.1.dev0`, con una única fuente en `src/rulepy/__init__.py` (versión dinámica de hatch). Licencia en formato PEP 639 (`license = "MIT"`, hatchling ≥ 1.27).
  - Desviación: `dev` **no** es un extra, sino que irá en `[dependency-groups]` (PEP 735) en la 1.3, porque las herramientas de desarrollo no deben poder instalarse con `pip install rulepy[dev]`.
  - Se excluyen del sdist `.claude/`, `CLAUDE.md` y `docs/PLAN.md`. El `[tool.uv.workspace]` se declarará en la fase 7, al añadir `rulepy-core`.
  - Verificado: `uv sync`, `import rulepy` y `uv build`; el wheel contiene solo `rulepy/` y los metadatos son correctos. `uv.lock` se versiona.
- [x] 1.3 Configuración de ruff (reglas `E,F,I,B,UP,SIM,RUF,D` con convención google), pyright estricto y pytest (marcador `slow`, perfiles de hypothesis `dev` y `ci`, cobertura).
  - Grupo `dev` (PEP 735): pytest 9.2, pytest-cov 7.1, hypothesis 6.170, ruff 0.17 y pyright 1.1.414.
  - pytest con `--strict-markers --strict-config`, `xfail_strict` y avisos convertidos en errores. Cobertura con ramas y `fail_under = 90` (se aplica con `--cov`).
  - Perfiles de hypothesis en `tests/conftest.py`: `dev` (50 ejemplos, por defecto) y `ci` (500), seleccionados con `HYPOTHESIS_PROFILE`.
  - ruff: línea de 100 caracteres; los docstrings no se exigen en `tests/` ni en `benchmarks/`. ruff 0.17 también formatea los bloques de código de los `.md`.
  - El hook de formato usa ahora el ruff de `.venv` (fijado en `uv.lock`) y recurre a `uvx` solo si no existe.
  - Tests de humo en `tests/test_package.py`: la versión coincide con los metadatos y `__all__` está declarado.
- [x] 1.4 Esqueleto de `src/rulepy/` con `errors.py` (`RulePyError`, `UnsupportedOperation`, `RuleSyntaxError`, `EvaluationError`) y `py.typed`.
  - Todas las excepciones admiten un `hint` (pista de corrección) y sobreviven a pickle con sus atributos, gracias a `__reduce__` y `_rebuild`. El nombre `_rebuild` debe mantenerse.
  - `RuleSyntaxError` es también `ValueError`; `UnsupportedOperation` es también `NotImplementedError`.
  - `UnsupportedOperation` y `EvaluationError` aceptan `rule` y `node`. `operation` es la etiqueta del operador en `docs/spec/ast.md`.
  - `RuleSyntaxError` usa posiciones 1-based (lanza `ValueError` si no lo son), guarda solo la línea problemática (`text`) y dibuja un circunflejo que respeta los tabuladores.
  - Desviación: ruff ignora D105 y D107 en todo el proyecto, porque los argumentos se documentan en el docstring de la clase. Contrapartida: D417 ya no compara la sección `Args:` con los parámetros del constructor, y pueden desincronizarse sin aviso.
  - Revisado por `api-reviewer`. Los hallazgos que se dejaron para más adelante están anotados en 2.3, 2.6 y 3.2.
- [x] 1.5 Test de "núcleo sin dependencias": `import rulepy` no importa numpy, polars, pandas, fuzzytool, sklearn ni z3.
  - `tests/test_core_dependencies.py`. Usa una **lista blanca** en vez de una lista de paquetes prohibidos: en un intérprete limpio (`python -I`), `import rulepy` solo puede cargar módulos de `sys.stdlib_module_names` o de `rulepy`, así que detecta cualquier dependencia accidental.
  - Un test de control comprueba que el detector sí señala las dependencias de hypothesis (`sortedcontainers`), para que el test principal no pueda pasar en falso.
  - Los módulos `_sysconfigdata_*` se tratan como estándar: se generan por plataforma y no aparecen en `stdlib_module_names`.
- [x] 1.6 CI con GitHub Actions: matriz Python 3.11–3.14 × Linux, macOS y Windows; lint, tipos, tests y cobertura.
  - `.github/workflows/ci.yml` con tres jobs:
    - `lint`: ruff y pyright;
    - `test`: 12 combinaciones con `--cov` y `HYPOTHESIS_PROFILE=ci`;
    - `build`: construye el wheel, lo instala solo en un entorno aislado, lo importa y lo sube como artefacto.
  - Configuración común: `UV_LOCKED=1` (falla si `uv.lock` está desactualizado), permisos de solo lectura y cancelación de ejecuciones obsoletas. `.github/dependabot.yml` actualiza las Actions cada mes.
  - **Todavía no se ha ejecutado en GitHub**: el repositorio no tiene remoto (la organización está pendiente). Verificado en local:
    - `actionlint` sin errores;
    - la suite pasa en macOS con 3.11, 3.12, 3.13 y 3.14;
    - el job `build` se reprodujo correctamente.
    - Windows y Linux quedan sin probar hasta la primera ejecución.
  - Las Actions están fijadas por versión mayor (`checkout@v5`, `setup-uv@v6`, `upload-artifact@v4`). Fijarlas por SHA queda para la fase 9, con la publicación.
- [x] 1.7 Esqueleto de mkdocs-material (mismo estilo que fuzzytool) y `docs/spec/README.md`.
  - `mkdocs.yml` (Material con modo claro y oscuro, mkdocstrings en estilo Google) con estas páginas: `docs/index.md`, `docs/reference/errors.md` (API generada) y `docs/spec/README.md`. `PLAN.md` queda fuera del sitio.
  - `docs/spec/README.md` fija las convenciones comunes: Kleene, etiquetas de operador, versión `rulepy_ast`, posiciones 1-based, subconjunto de regex y determinismo. Está en inglés porque la lee también `rust-engineer`.
  - Desviación: `docs` es un **grupo de dependencias** (mkdocs-material 9.8 y mkdocstrings 1.0), no un extra, por la misma razón que `dev`. Se ha retirado el extra vacío `docs`.
  - Nuevo job `docs` en CI: `mkdocs build --strict`.
  - ⚠️ **Material for MkDocs termina su soporte el 5 de mayo de 2027**; el sucesor es Zensical, compatible con `mkdocs.yml`. Ver 9.1. (También afecta a fuzzytool.)

**Aceptación**: `uv sync && uv run pytest && uv run ruff check . && uv run pyright` en verde, en local y en CI.

> **Estado (2026-10-30)**: todo verde en local (33 tests, 100 % de cobertura, macOS con Python 3.11–3.14) y documentación en modo estricto correcta. Falta la verificación en CI, que llegará con la primera ejecución en GitHub (pendiente de la organización).

---

### Fase 2. Núcleo: AST, lógica, reglas y motor secuencial · v0.1 · tamaño L

**Objetivo**: reglas de negocio sin estado, con explicación completa. Es la base de todo lo demás.

**AST (`core/expr.py`)**
- [x] 2.1 Nodos inmutables con igualdad estructural:
  - `Field(path)` y `Const`;
  - `Compare` (`== != < <= > >=`);
  - `And`/`Or` n-arios (aplanados) y `Not`;
  - `In`, `Between`, `IsNull`, `Matches`, `StartsWith`/`EndsWith`/`Contains`;
  - aritmética `+ - * /` y `abs`.
  - Dividida en dos subtareas, para que cada una quepa en un commit revisable:
    - [x] 2.1a Base `Expr` (inmutable, `children`, `walk`, `key`, `same_as`), `Field`, `Const`, `Compare`, `And`/`Or` (n-arios, aplanados), `Not` y validación al construir. Incluye `docs/spec/ast.md` con **todos** los nodos.
      - Constructores con argumentos sueltos: `Field("pedidos", 0, "total")` y `And(a, b, c)`, como el `And(a, b)` de fuzzytool. `key()` pasa a llamarse `structural_key()`.
      - Tipos de nodo `condition`/`value`/`any` (antes `boolean`, renombrado para encajar con la lógica difusa). `Compare` rechaza condiciones como operandos y `Const(None)` (pista: `IsNull`). `Const(True)` es de tipo `any`.
      - `Const` normaliza a tipos exactos (numpy, Enum, subclases) y rechaza NaN, enteros de más de 64 bits y datetimes con nanosegundos.
      - Unión cerrada `Node`, `Tag` y `TAGS` por clase. Un test comprueba que todos los tags están en `docs/spec/ast.md`.
      - Nodos con hash por identidad y `weakref_slot`. `structural_key()` lanza `TypeError` ante datos que no sean escalares.
      - `ExpressionError(RulePyError, TypeError, ValueError)` con `rule` y `node`.
      - **Cambio de semántica tras la revisión**: comparar valores de grupos distintos (p. ej. `str` frente a `int`) da `UNKNOWN`, con el motivo en la traza. Antes lanzaba un error, lo que hacía depender el resultado del orden de evaluación y abortaba lotes enteros. En modo `strict` se lanza `TypeMismatchError` (2.5). La especificación concreta además los nodos de la 2.1b y las notas para polars (4.3).
      - Pendiente: cachear `structural_key()` si Rete o la auditoría lo necesitan (se recalcula de forma recursiva; si se cachea en un slot, excluirlo de `fields()` con `init=False`).
    - [x] 2.1b `In`, `Between`, `IsNull`, `Matches`, `StartsWith`/`EndsWith`/`Contains`, aritmética, `Abs` y `Neg`.
      - Las pruebas de texto son un único nodo, `StringTest(op, operand, text)`, con `text` literal (`str`).
      - Los nodos que reciben valores rechazan condiciones y `Const(None)` (salvo `IsNull`). La aritmética rechaza constantes no numéricas.
      - `In`:
        - acepta cualquier colección, incluido un `set`, pero no `str`, `bytes` ni mapas;
        - rechaza `None` y la mezcla de grupos de tipos;
        - guarda los valores ordenados y sin duplicados, de forma determinista (`1` y `1.0` se conservan ambos).
      - `Between` rechaza límites constantes de grupos distintos o booleanos.
      - `Matches` y `StringTest` normalizan las subclases de `str`.
      - Desviación: `Matches` solo comprueba que el patrón compila; el subconjunto portable Python/Rust queda para la 3.6.
      - Las estrategias de hypothesis generan ya el AST completo.
      - Revisado por `api-reviewer`, sin bloqueantes. Se corrigieron tres fallos: orden de `In` con `1`/`1.0`, `In(x, "abc")` partido en caracteres, y subclases de `str` que rompían `structural_key()`.
- [x] 2.2 Proxy `F` con acceso por atributo e índice (`F.cliente.antiguedad`, `F["clave con espacios"]`) y construcción con operadores `&`, `|` y `~`.
  - **Hecha junto con la 2.3**: con `F.x > 5` devolviendo un nodo pero sin proteger `bool()`, `0 < F.x < 10` se habría evaluado en silencio como `F.x < 10`.
  - Operadores: comparaciones, `&`/`|`/`~`, `+ - * /`, `-x` y `abs()` (todos con versión reflejada). Métodos `is_in`, `between`, `is_null`, `matches`, `starts_with`, `ends_with`, `contains` y `abs`.
  - Rutas: `F["a", 0, "b"]`. `F` se exporta desde `rulepy`.
  - Tras la revisión de `api-reviewer`:
    - `&` y `|` rechazan `bool` de Python (antes `(F.p > 5) & (F.producto.tag == "x")` se convertía en `& False` sin avisar);
    - los nodos y `F` no son iterables (antes `list(F.x)` era un bucle infinito por el protocolo de secuencias heredado);
    - `__array_ufunc__ = None` y `__pandas_priority__` para numpy y pandas a la izquierda;
    - `Field` rechaza los nombres reservados y los espacios de nombres al estilo polars (`str`, `dt`, ...);
    - llamar a una ruta (`F.x.isin(...)`) sugiere el método correcto.
  - Único caso silencioso que queda, documentado: `F.a == F.b.kind`.
- [x] 2.3 Trampas de Python resueltas (hecha con la 2.2; ver `docs/pitfalls.md`):
  - `__bool__` lanza un error claro, lo que impide `a and b` y comparaciones encadenadas como `0 < F.x < 10`;
  - `__eq__` devuelve un nodo (la igualdad estructural es `same_as()`). **El hash sigue siendo por identidad**: un hash estructural no funciona, porque dict y set llaman a `__eq__` cuando coinciden los hashes y `bool(Compare)` lanzaría un error. Para claves estructurales se usa `structural_key()`.
    - Al definir `__eq__`, Python pone `__hash__ = None`: hay que declarar `__hash__ = object.__hash__` en `Expr` y comprobar con un test que todos los nodos son hashables.
    - También hay que sobrecargar `__ne__`.
    - Documentar en "trampas comunes" lo que deja de funcionar: `nodo in lista` (salvo que sea el mismo objeto), `list.index`/`remove`/`count`, `==` entre tuplas o listas de nodos, `assert n1 == n2` en pytest y `WeakKeyDictionary`.
  - `repr` legible.
  - *Nota de 1.4*: cuando existan nodos reales, los tests de `errors` que comparan con `vars(a) == vars(b)` fallarán si `node` es un `Expr` (porque `__eq__` devuelve un nodo). En ese caso se compara con `same_as`. El mal uso de `__bool__` lanza `ExpressionError` (que ya es `TypeError`). Los tests de nodos comparan con el helper `same_nodes` (por identidad) o con `same_as`, nunca con `==`.

**Lógica (`core/logic.py`)**
- [x] 2.4 Protocolo `Logic` (`and_`, `or_`, `not_`, `from_bool`, `is_true`, `TRUE`/`FALSE`/`UNKNOWN`) con registro por nombre.
- [x] 2.5 `BooleanLogic` de Kleene con 3 valores. Un campo ausente o `None` da `UNKNOWN`, y la regla no se dispara. Hay un modo `strict=True` que lanza una excepción.
  - Hecha junto con la 2.4.
  - `Truth`: `Enum` ordenado (`FALSE < UNKNOWN < TRUE`) que no se puede usar como `bool` ni compararse con números.
  - `Logic[TruthT]`: protocolo genérico, **provisional hasta la 1.0**. `all_` y `any_` reciben una `Sequence` ya evaluada (nunca un generador), para que el resultado no dependa del orden; tienen implementación por defecto en el protocolo.
  - `GradedLogic` (`degree`, `from_degree`): capacidad opcional para las funciones basadas en grados de la fase 5.
  - Registro: `get_logic(nombre, **parámetros)`, `register_logic` con cerrojo (Python sin GIL) y `unregister_logic`. `boolean` es integrada: no se puede sustituir ni desregistrar.
  - Las lógicas del mismo tipo y parámetros son iguales, y deben ser inmutables y sin estado.
  - `is_null` único en `_compat` (NaN de numpy y `Decimal`, `pd.NA`, `NaT`).
  - Errores nuevos: `TypeMismatchError`, `MissingFieldError` y `RegistryError`.
  - Desviación: el modo `strict` vive en el **evaluador** (2.6), no en `BooleanLogic`. La lógica lanza `TypeMismatchError` y el evaluador decide entre `UNKNOWN` (con el motivo en la traza) y el error.
  - Revisado por `api-reviewer`, sin bloqueantes. **Pendiente de confirmar por el autor**: `UNKNOWN` en lógica difusa como intervalo `[0, 1]` (ver `docs/spec/ast.md`).
  - *Nota de 2.5*: al leer un campo, aplicar `is_null` una sola vez para normalizar los nulos a `None`. Pasar siempre listas ya evaluadas a `all_`/`any_`. El motivo de `UNKNOWN` (nulo, ausente, tipos incompatibles) va en el nodo de la traza, no en `Truth`.
  - *Nota de 2.1a*: los tipos incompatibles dan `UNKNOWN` con el motivo en la traza. En modo `strict`, `TypeMismatchError(EvaluationError)` con la ruta del campo y los dos tipos. Grupos de comparación según `docs/spec/ast.md` (número = `numbers.Real` sin `bool`).

**Evaluación y reglas**
- [x] 2.6 Evaluador de referencia `core/evaluate.py`, sobre `Mapping`, dataclasses, objetos con atributos y modelos Pydantic (por duck typing, sin depender de Pydantic).
  - `evaluate` (con sobrecargas de tipo), `evaluate_tree` y `evaluate_value`. Devuelven un árbol `NodeResult` (`node`, `value`, `reason`, `children`) que será la base de la traza (2.10). Un `Between` se representa con sus dos medias comparaciones.
  - Motivos **estructurados**: `Reason(code, paths, types, detail)` con 8 códigos normativos para todos los backends (`docs/spec/ast.md`, "Results and reasons"). Solo aparecen en UNKNOWN o nulo.
  - Sin cortocircuito. El modo `strict` lanza el primer error en postorden de izquierda a derecha.
  - Lectura de datos especificada (nueva sección "Reading data"):
    - mapas por clave (sin `__missing__`);
    - filas con `keys()`;
    - registros tipo namedtuple;
    - atributos **públicos de datos**;
    - índices sobre secuencias con longitud, incluidos arrays de numpy.
  - Tras la revisión de `api-reviewer`:
    - **fallo de seguridad corregido**: una ruta como `F.o.__class__.__init__.__globals__.os.environ` leía variables de entorno; ahora no se leen atributos con `_` ni de clases, módulos o escalares;
    - `Decimal` cuenta como número;
    - `div` convierte antes a float64, como polars y Rust;
    - los enteros de los datos fuera de 64 bits son nulos;
    - `datetime64` se convierte a `datetime`;
    - una propiedad que lanza da `read_error`;
    - `format_path` ya no es ambigua.
  - Las notas de la especificación sobre polars recogen `Float32` y `Decimal`.  - *Nota de 1.4*:
    - restringir la anotación de `node` en `EvaluationError` y `UnsupportedOperation` a `Expr | str | None` (Python pasa el nodo; los demás backends pasan su texto);
    - errores nuevos previstos: campo ausente en modo estricto (2.5), `HitPolicyError(EvaluationError)` con `rules: tuple[str, ...]` para `UNIQUE` (2.8), y un índice de fila para `evaluate_many`.
- [ ] 2.7 `Rule(name, when, then, priority=0, tags=(), description=None, metadata={})`. `then` puede ser un dict (payload de decisión), una función `(ctx) -> None` o una lista de acciones declarativas (`Set`, `Append`, `Emit`).
  - *Nota de 2.2*: `Rule` necesita `eq=False` o su propio `__eq__`, porque comparar dos reglas compararía sus `when` y lanzaría un error. En 2.6 y 2.7, nunca usar duck typing (`hasattr`/`getattr`), `==` ni `if x:` sobre algo que pueda ser un nodo: comprobar antes `isinstance(x, Expr)`.
- [ ] 2.8 `RuleSet` con políticas de acierto al estilo DMN:
  - `FIRST`, `PRIORITY`, `UNIQUE`, `ANY`, `COLLECT` (con agregadores `sum`, `min`, `max`, `count` y `list`), `RULE_ORDER`;
  - la política `UNIQUE` lanza un error si se disparan dos reglas.
- [ ] 2.9 `engines/sequential.py`: `ruleset.evaluate(data) -> Decision` y `evaluate_many(iterable)`.

**Explicación (`core/trace.py`)**
- [ ] 2.10 Árbol de traza con el valor de verdad de cada nodo y los valores de campo resueltos.
- [ ] 2.11 Métodos de `Decision`:
  - `.explain()` (texto y estructura): por qué se disparó cada regla y por qué no se disparó el resto, destacando la subcondición mínima que falla;
  - `.why_not(rule_name)`;
  - una **pista contrafactual** en las comparaciones ("se dispararía si `total > 1000`; ahora vale 800").
- [ ] 2.12 `summary()` de `RuleSet` (estilo fuzzytool).

**Calidad**
- [ ] 2.13 `tests/strategies.py`: estrategias de hypothesis para generar AST, datos (con nulos, NaN y unicode) y reglas.
  - Empezado en 2.1a (`scalars`, `fields`, `value_exprs`, `compares`, `conditions`). Faltan los datos y las reglas.
- [ ] 2.14 Propiedades: De Morgan, doble negación, tablas de verdad de Kleene, conmutatividad y asociatividad de `And`/`Or`, idempotencia de aplanar.
- [ ] 2.15 Snapshots de `explain()`.
- [ ] 2.16 `benchmarks/run.py` con la suite `eval` (100k registros × 100 reglas), comparada con rule-engine y business-rules. Se guarda como línea base.

**Aceptación**: el ejemplo de "descuento VIP" de la documentación funciona; cobertura ≥ 95 % en `core/`; `api-reviewer` sin hallazgos bloqueantes.

**Riesgos**: la sobrecarga de `==` en nodos confunde a los usuarios. Se mitiga con errores explícitos y documentación de "trampas comunes".

---

### Fase 3. Reglas como datos · v0.2 · tamaño M

**Objetivo**: que personas que no programan editen las reglas, y que estas se versionen como ficheros.

- [ ] 3.1 `docs/spec/ast.md` y `ast-schema.json` (JSON Schema) con `rulepy_ast: 1`: cada nodo con su forma, semántica y tratamiento de nulos.
  - *Nota de 2.1a*: el catálogo y la semántica ya están en `ast.md` (versión 0). Falta la forma JSON, incluida la codificación de `inf`/`-inf` (JSON no la tiene), de las fechas y de las zonas horarias.
- [ ] 3.2 `to_dict`/`from_dict` para nodos, reglas y rulesets; validación contra el esquema.
  - *Nota de 1.4*: los errores de esquema o de versión se localizan por ruta JSON, no por línea y columna. Hará falta un error específico (p. ej. `SchemaError(RuleSyntaxError)` con `path`), así como un error para nombres desconocidos en los registros. Las posiciones de PyYAML empiezan en 0 y hay que sumarles 1.
- [ ] 3.3 Cargador y volcador YAML (extra `yaml`, `safe_load`) y JSON. Las acciones con funciones se referencian por nombre mediante un registro (`@action("enviar_alerta")`).
- [ ] 3.4 DSL de texto con parser Pratt propio:
  - `rule_from_text("IF cliente.antiguedad > 5 AND pedido.total > 1000 THEN descuento = 0.15")`;
  - sintaxis alineada con `fuzzytool.rule_from_text`;
  - errores con línea y columna.
- [ ] 3.5 Tablas de decisión (`io/tables.py`) desde una lista de filas, CSV o un DataFrame, con políticas de acierto, celdas de rango (`[18..65)`) y comodín `-`.
- [ ] 3.6 Restricción de regex (*nota de 2.1b*: `$` difiere entre Python, donde también encaja antes de un salto de línea final, y Rust; hay que rechazarlo o reescribirlo como `\z`): se validan los patrones contra el subconjunto del crate `regex` (sin *lookaround* ni *backreferences*) y se documenta el riesgo ReDoS en Python.
- [ ] 3.7 Propiedad de ida y vuelta: `parse(serialize(x)) == x` para JSON, YAML y texto (mediante `parity-tester`).

**Aceptación**: todos los ejemplos de la fase 2 existen en Python, YAML y texto con resultados idénticos.

---

### Fase 4. Validación de datos · v0.3 · tamaño M

**Objetivo**: competir con pandera reutilizando las mismas reglas.

- [ ] 4.1 `Rule(..., severity="error"|"warning"|"info")` y `RuleSet.validate(data) -> ValidationReport`.
- [ ] 4.2 Evaluador por filas (sin dependencias) para listas de dicts y objetos.
- [ ] 4.3 `validation/polars_backend.py`: compila el AST a `pl.Expr` con la misma semántica de Kleene. pandas se soporta por conversión y, si hace falta, con un backend nativo.
- [ ] 4.4 Reglas de conjunto de datos:
  - unicidad, claves foráneas entre tablas;
  - agregados (`F.edad.mean() < 50`, `count`);
  - reglas condicionales entre columnas.
- [ ] 4.5 `ValidationReport`:
  - recuentos por regla y filas que fallan con valores de muestra;
  - `to_polars()`/`to_pandas()`, `to_markdown()`/`to_html()`;
  - `explain(row)`.
- [ ] 4.6 Paridad entre el backend polars y el evaluador de referencia sobre datos aleatorios con nulos (`parity-tester`).
- [ ] 4.7 Benchmark `validation`: 10M filas comparadas con pandera y el evaluador por filas.

**Aceptación**: paridad del 100 % en las propiedades y el benchmark publicado en `benchmarks/results/`.

---

### Fase 5. Lógica difusa (fuzzytool) y auditoría estática · v0.4 · tamaño L

**Objetivo**: los dos grandes diferenciadores.

**Integración con fuzzytool (extra `fuzzy`)**
- [ ] 5.1 (*nota de 2.5*: implementar `GradedLogic` con valores de verdad como intervalos `[lo, hi]` si el autor confirma la propuesta de `ast.md`; registrarla como `get_logic("fuzzy", tnorm=..., snorm=..., complement=...)`, con igualdad por parámetros) `FuzzyLogic(tnorm="min", snorm="max", complement="standard")`, que resuelve las conectivas con `fuzzytool.norms.get_tnorm/get_snorm/get_complement`. Los predicados nítidos valen {0, 1}.
- [ ] 5.2 Nodo `FuzzyTerm`, que envuelve una `fuzzytool.sets.Proposition` (variable y término) y se evalúa con `Antecedent.eval(inputs, tnorm, snorm, complement=...)`. Se serializa con `Variable.to_dict()`. *Nota de 2.1a*: debe redefinir `structural_key()`, porque `Proposition` no tiene igualdad estructural.
- [ ] 5.3 Interoperabilidad de operadores:
  - en RulePy, `fz(term)` y `__rand__`/`__ror__`;
  - **cambio en fuzzytool 0.8**: `Antecedent.__and__/__or__` devuelven `NotImplemented` cuando reciben tipos ajenos, para que `score["poor"] & (F.x > 2)` funcione en ambos órdenes.
- [ ] 5.4 Comparaciones suaves: `F.x.gt(5, soft=2)` produce una rampa de pertenencia (un *hedge* sin definir variables).
- [ ] 5.5 Disparo con grado:
  - `Decision` registra el grado de cada regla;
  - políticas `MAX_DEGREE` y `WEIGHTED`, y umbral α para ejecutar acciones.
- [ ] 5.6 Un sistema de fuzzytool como acción: `then=Infer(sistema, inputs={"score": F.score}, output="premium")`, compatible con Mamdani, TSK y tipo 2.

**Auditoría (`audit/`, extra `audit` para z3)**
- [ ] 5.7 Abstracción por intervalos y conjuntos por campo sobre el AST, como dominio abstracto sencillo.
- [ ] 5.8 Detección de:
  - condiciones insatisfacibles (`x > 5 & x < 3`);
  - reglas **ensombrecidas** (nunca se disparan con `FIRST`/`PRIORITY`);
  - solapamientos con `UNIQUE`;
  - **contradicciones** (misma región, salidas incompatibles);
  - **huecos de cobertura** en tablas de decisión sobre dominios acotados;
  - campos y salidas sin usar.
- [ ] 5.9 Backend z3 opcional para lo que los intervalos no deciden (aritmética, relaciones entre campos).
- [ ] 5.10 `audit(ruleset)` con un informe de formato análogo al de `fuzzytool.audit`. En reglas difusas se delega en `fuzzytool.audit`.
- [ ] 5.11 Corpus de rulesets con defectos sembrados: precisión y exhaustividad del 100 % en el fragmento decidible por intervalos.

**Aceptación**: un ejemplo de riesgo de crédito que mezcla reglas nítidas y difusas, explicado y auditado de principio a fin.

**Riesgos**: la explosión combinatoria en la auditoría. Se mitiga con límites configurables y el modo "desconocido" honesto.

---

### Fase 6. Sistema experto (encadenamiento hacia delante) · v0.5 · tamaño XL

**Objetivo**: superar a experta y durable_rules en ergonomía y explicación. La velocidad llega en la fase 7.

- [ ] 6.1 `Fact`: clase base sobre dataclass con tipos, identificador y marca temporal. Adaptador para modelos Pydantic.
- [ ] 6.2 Patrones con variables y joins:
  ```python
  @rule(Paciente(fiebre=V.f > 38, id=V.p), ~Diagnostico(paciente=V.p))
  ```
  Incluye negación (`not exists`), `exists`, condiciones de test y, más adelante, `accumulate`/`collect`.
- [ ] 6.3 Memoria de trabajo: `assert_`, `modify` y `retract`, con eventos.
- [ ] 6.4 Agenda:
  - activaciones y refracción;
  - estrategias de conflicto `salience`, `LEX`, `MEA`, `depth`/`breadth`, todas deterministas.
- [ ] 6.5 Matcher ingenuo (oráculo) y **Rete incremental en Python** (memorias alfa e índices hash en beta). Paridad entre ambos con `parity-tester`.
- [ ] 6.6 Mantenimiento de verdad: `ctx.assert_logical()`, que retira el hecho cuando desaparece su soporte.
- [ ] 6.7 (*nota de 2.5*: la atenuación `CF(premisa) × CF(regla)` y la combinación de evidencias son una capacidad opcional aparte, p. ej. `EvidenceLogic`, no miembros de `Logic`) Hechos con grado o certeza que se propagan con la `Logic` activa (nítida, difusa o factores de certeza al estilo MYCIN como `Logic` adicional).
- [ ] 6.8 Explicación:
  - árbol de derivación de cada hecho ("¿por qué se cree X?");
  - `why_not(patrón)`;
  - registro de disparos reproducible.
- [ ] 6.9 Encadenamiento hacia atrás básico: `engine.prove(Objetivo(...))` sobre las mismas reglas.
- [ ] 6.10 Benchmark `expert`: Miss Manners y Waltz contra experta, durable_rules y clipspy.

**Aceptación**: los ejemplos clásicos (diagnóstico, configuración) funcionan; hay paridad entre el matcher ingenuo y Rete; los benchmarks están publicados.

**Riesgos**: la complejidad de Rete con negación y retracción. Se mitiga con el oráculo ingenuo y con tests diferenciales desde el primer día.

---

### Fase 7. `rulepy-core` en Rust · v0.6 · tamaño XL

**Objetivo**: velocidad sin cambiar la semántica. Trabajo del subagente `rust-engineer`.

- [ ] 7.1 Workspace de Cargo:
  - `crates/rulepy-core` (Rust puro, sin PyO3);
  - `crates/rulepy-py` (PyO3, `abi3-py311`) → paquete `rulepy-core` y módulo `rulepy_core`.
- [ ] 7.2 Deserialización del AST con serde desde el formato de `docs/spec/`, con verificación de versión.
- [ ] 7.3 Compilador del AST a bytecode y una VM con Kleene, y `FuzzyLogic` para las normas estándar.
- [ ] 7.4 Evaluación por lotes columnar sobre Arrow (sin copias desde polars y pyarrow mediante `pyo3-arrow`), en paralelo con rayon y liberando el GIL.
- [ ] 7.5 Rete en Rust:
  - almacén de hechos tipado con esquemas registrados desde las clases `Fact`;
  - acciones declarativas ejecutadas en Rust;
  - funciones de Python como ruta lenta documentada.
- [ ] 7.6 `rulepy/_backend.py`: autodetección de `rulepy_core` y `RULEPY_BACKEND=python|rust` para forzar uno u otro.
- [ ] 7.7 La suite completa se ejecuta con los dos backends (fixture parametrizada), con tests diferenciales de hypothesis Python ↔ Rust.
- [ ] 7.8 Wheels con `maturin-action`:
  - manylinux x86_64 y aarch64, macOS x86_64 y arm64, Windows;
  - wheel *free-threaded* 3.14t.
- [ ] 7.9 Benchmarks. Los objetivos son metas, no promesas:
  - evaluación por lotes ≥ 10× sobre la referencia Python;
  - Miss Manners mejor que clipspy;
  - batch competitivo con zen-engine.

**Aceptación**: la suite está en verde con ambos backends; las mejoras están medidas con `/bench` (mediana y rango intercuartílico) y publicadas.

**Riesgos**:
- coste de cruzar la frontera Python ↔ Rust en cada hecho, que se mitiga con lotes y Arrow;
- compilación en 3 sistemas operativos, que se mitiga con CI desde 7.1 y con la ruta Python siempre disponible.

---

### Fase 8. Explicaciones de ML y guardarraíles · v0.7 · tamaño L

- [ ] 8.1 Extracción exacta desde `DecisionTreeClassifier`/`Regressor` de sklearn: un camino por hoja, simplificado y fusionado.
- [ ] 8.2 Árbol sustituto para cualquier modelo (`method="surrogate_tree"`), con profundidad y soporte mínimo configurables.
- [ ] 8.3 RuleFit y Skope mediante imodels (opcional), convertidos a reglas de RulePy.
- [ ] 8.4 **Extracción difusa** con `fuzzytool` (`chi`, ANFIS y particiones por clustering), convertida en reglas de RulePy con `FuzzyLogic`.
- [ ] 8.5 Métricas: fidelidad, cobertura, precisión por regla, complejidad (longitud) y estabilidad ante bootstrap.
- [ ] 8.6 Explicación por instancia: la regla o reglas que cubren el caso y el contrafactual que proporciona la traza.
- [ ] 8.7 Guardarraíles:
  - `Guarded(estimator, ruleset)`, compatible con sklearn (`fit`, `predict`, `predict_proba`, pipelines);
  - las reglas pueden anular o vetar la predicción y registran el motivo;
  - auditoría del conjunto de reglas más el modelo.
- [ ] 8.8 Reproducir las cifras de fidelidad de imodels en adult y german credit, y comparar.

**Aceptación**: un notebook de extremo a extremo: entrenar, extraer, auditar, aplicar guardarraíles y explicar.

**Oportunidad de investigación**: guardarraíles neuro-simbólicos difusos (modelo + reglas difusas auditables) como artículo.

---

### Fase 9. Documentación, comparativa y publicación · v1.0 · tamaño M

- [ ] 9.0 Migrar la documentación de Material for MkDocs (fin de soporte: 5 de mayo de 2027) a **Zensical**, que lee `mkdocs.yml`. Debe hacerse antes de esa fecha, sea cual sea la fase en curso; conviene coordinarlo con fuzzytool.
- [ ] 9.1 Sitio de documentación:
  - tutoriales por caso de uso;
  - "trampas comunes";
  - referencia de la API con mkdocstrings;
  - especificación del AST.
- [ ] 9.2 Matriz de funcionalidades frente a cada rival y página de benchmarks reproducible (scripts y resultados).
- [ ] 9.3 Notebooks en `examples/`, ejecutados en CI con nbconvert.
- [ ] 9.4 Política de estabilidad de la API y de deprecación (dos versiones menores de aviso).
- [ ] 9.5 Publicación con *trusted publishing* (OIDC de GitHub) en PyPI, para `rulepy` y `rulepy-core`.
- [ ] 9.6 Paper en JOSS (`paper.md`), DOI en Zenodo y `CITATION.cff`.

---

## 4. Prácticas transversales

**Estrategia de tests**

| Tipo | Herramienta | Dónde |
|---|---|---|
| Unitarios | pytest | `tests/<módulo>/` |
| Propiedades algebraicas | hypothesis | `tests/properties/` |
| Paridad entre backends | hypothesis (`parity-tester`) | `tests/parity/` |
| Snapshots de explicaciones | pytest + ficheros `.txt` | `tests/snapshots/` |
| Regresión de rendimiento | `/bench` contra `baseline.json` | `benchmarks/` |
| Rust | `cargo test`, `proptest` | `crates/*/tests/` |

**Versionado**: SemVer en 0.x, con una versión menor por fase. `rulepy` y `rulepy-core` comparten versión y se comprueba la compatibilidad al importar.

**Seguridad**:
- nada de `eval`/`exec` y solo `safe_load`;
- regex restringidas y límites de profundidad del AST y de tamaño de la entrada al parsear;
- las acciones con funciones solo se resuelven desde un registro explícito, nunca por ruta de importación arbitraria leída de un YAML.

**Rendimiento**: en Python se prioriza que el código sea correcto y claro. Las optimizaciones van a la fase 7 o a polars. Nadie afirma una mejora sin `/bench`.

## 5. Dependencias entre fases

```
0 → 1 → 2 ─┬→ 3 ─┬→ 4
           │     └→ 5 (necesita 3 para serializar FuzzyTerm)
           └→ 6 (necesita 2; usa 5.1 si hay hechos difusos)
2, 3, 4, 6 → 7 (Rust necesita la especificación de 3 y los oráculos de 2, 4 y 6)
2, 5 → 8
todas → 9
```

Las fases 4, 5 y 6 pueden avanzar en paralelo una vez cerrada la 3.

## 6. Riesgos globales

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Alcance excesivo (4 casos, difuso y Rust) | Alto | Fases publicables, núcleo común y el Rust a partir de la fase 7 |
| Divergencia semántica Python ↔ polars ↔ Rust | Alto | Especificación versionada y tests diferenciales obligatorios |
| Rete con negación y retracción incorrecto | Alto | Oráculo ingenuo y hypothesis |
| Wheels en 3 sistemas operativos | Medio | CI desde 7.1 y ruta Python siempre disponible |
| La API de fuzzytool cambia | Bajo | Mismo autor, adaptador aislado en `rulepy/fuzzy/adapter.py` y rango de versión fijado |
| Afirmaciones de rendimiento no sostenibles | Medio | Benchmarks reproducibles y publicados con mediana y rango intercuartílico |

## 7. Cómo trabajar con Claude en este plan

1. `/plan-task` (o `/plan-task 2.3`): implementa la siguiente tarea con tests primero, puertas de calidad y casilla marcada.
2. Los operadores nuevos se añaden siempre con `/add-operator`.
3. Los cambios en la API pública pasan por el subagente `api-reviewer`, y los cambios con varios backends por `parity-tester`.
4. Todo lo que hay bajo `crates/` lo hace `rust-engineer`.
5. Antes de afirmar cualquier mejora de rendimiento se ejecuta `/bench`.
