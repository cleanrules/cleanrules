# RulePy

Motor de reglas para Python que cubre cuatro casos de uso con un único núcleo: reglas de negocio, sistema experto (encadenamiento hacia delante), validación de datos y explicaciones de modelos de ML. Admite lógica nítida y difusa (integra `fuzzytool`). El núcleo de referencia está en Python y, más adelante, habrá un acelerador opcional en Rust (`rulepy-core`).

- **Hoja de ruta y estado**: [docs/PLAN.md](docs/PLAN.md) es la fuente de verdad. Antes de empezar cualquier tarea, léela y localiza la fase y la tarea.
- **Contratos**: [docs/spec/](docs/spec/) contiene las especificaciones que comparten Python y Rust, empezando por el formato del AST. Si cambia el comportamiento, primero se actualiza la especificación.
- **Fase actual**: 0 completada (especificaciones). Siguiente: fase 1 (andamiaje).

## Comandos

```bash
uv sync --all-extras                 # entorno de desarrollo
uv run pytest                        # tests (incluye hypothesis)
uv run pytest -m "not slow"          # ciclo rápido
uv run pytest --cov                  # con cobertura (falla por debajo del 90 %)
HYPOTHESIS_PROFILE=ci uv run pytest  # búsqueda de hypothesis más profunda (500 ejemplos)
uv run ruff check . && uv run ruff format --check .
uv run pyright                       # tipado estricto
uv run maturin develop -m crates/rulepy-py/Cargo.toml   # compilar el núcleo Rust (fase 7+)
cargo test --workspace && cargo clippy --workspace -- -D warnings
uv run python benchmarks/run.py      # benchmarks (fase 2+)
```

Un hook (`.claude/hooks/format.sh`) formatea cada `.py` y `.rs` tras editarlo, así que no hace falta formatear a mano.

## Invariantes de arquitectura (no romper)

1. **Las condiciones son datos.** Una condición es un AST (`rulepy.core.expr`), nunca una lambda opaca. Las *acciones* sí pueden ser funciones de Python. Todo lo demás (serializar, explicar, vectorizar, auditar, compilar a Rust) depende de esto.
2. **Cada nodo del AST funciona en todos los backends** (evaluador Python, explicación, serialización, compilador polars, auditoría y Rust) o lanza `UnsupportedOperation` de forma explícita. Para añadir un operador, usa la skill `/add-operator`.
3. **La lógica es intercambiable.** El evaluador nunca usa `and`/`or`/`not` de Python sobre los valores de verdad: todo pasa por el protocolo `Logic` (`BooleanLogic` de Kleene con 3 valores, `FuzzyLogic` con normas de fuzzytool).
4. **El núcleo no tiene dependencias en tiempo de ejecución.** numpy, polars, pandas, fuzzytool, scikit-learn, z3, PyYAML y `rulepy-core` son *extras*, y se importan de forma perezosa dentro de su módulo. Hay un test que comprueba que `import rulepy` no los carga.
5. **Python es el oráculo.** La implementación Python define la semántica. Rust, polars y Rete deben dar exactamente el mismo resultado, y lo verifican tests diferenciales con hypothesis. Un cambio de semántica toca ambos lados en el mismo PR.
6. **El formato del AST está versionado** (`"rulepy_ast": <n>` en `docs/spec/ast.md` y `ast-schema.json`). Es el contrato con Rust y con los ficheros YAML/JSON de los usuarios.
7. **Determinismo.** Las mismas entradas producen el mismo orden de disparo y la misma traza. La resolución de conflictos es estable y no depende del orden de iteración de `set` ni del `hash` de objetos.
8. **Todo resultado se explica.** Cada motor devuelve un objeto con `.explain()`. Las explicaciones forman parte de la API pública y tienen tests de *snapshot*.
9. **Nada de `eval`/`exec`.** El DSL de texto usa un parser propio, y YAML se carga solo con `safe_load`. Las expresiones regulares se limitan al subconjunto compatible con el crate `regex` de Rust (sin *lookaround* ni *backreferences*).

## Coherencia con fuzzytool

fuzzytool es del mismo autor, y RulePy debe sentirse como su hermana:
- operadores `&` (AND/t-norma), `|` (OR/s-norma) y `~` (NOT);
- conectivas y estrategias resueltas por nombre mediante registros y `Protocol`s;
- verbos `explain()`, `audit()`, `summary()` y `rule_from_text()`;
- docstrings en estilo Google con `Args:`/`Returns:`.

**No se reimplementa lógica difusa en RulePy**: se usa `fuzzytool.norms.get_tnorm/get_snorm/get_complement` y `fuzzytool.sets.Antecedent.eval(...)`.

Un detalle conocido: `fuzzytool.sets.Antecedent.__and__` no devuelve `NotImplemented` cuando recibe un tipo ajeno. Por eso `score["poor"] & (F.x > 2)` construye un `And` de fuzzytool inválido. Hay que poner la expresión de RulePy a la izquierda o envolverla con `fz(score["poor"])`. La corrección en fuzzytool está prevista en el plan (fase 5).

## Convenciones

- Python ≥ 3.11, estructura `src/`, `pyright` en modo estricto y `ruff` (configuración en `pyproject.toml`).
- **Todo lo que se publica va en inglés**: código, identificadores, docstrings, mensajes de error, documentación de usuario y commits. Este fichero y `docs/PLAN.md` están en español.
- Errores: todos heredan de `rulepy.errors.RulePyError`, y los mensajes indican qué regla o nodo falla y cómo corregirlo.
- API pública: solo lo que exporta `__all__`. Cualquier cambio en ella actualiza `CHANGELOG.md` (formato Keep a Changelog) y la documentación.
- Commits: Conventional Commits (`feat(core): ...`, `fix(rete): ...`). Un commit o PR por tarea del plan.
- Tests: `tests/` refleja la estructura de `src/rulepy/`. Las propiedades algebraicas y la paridad entre backends se prueban con hypothesis. Las estrategias compartidas viven en `tests/strategies.py`.
- Los tests lentos o de benchmark se marcan con `@pytest.mark.slow`.
- No usar `print` en la librería. El logging va por `logging.getLogger("rulepy")`.

## Definición de terminado (cada tarea)

1. Los tests nuevos fallaban antes del cambio y ahora pasan, y pasa la suite completa.
2. `ruff`, `pyright` y (si hay Rust) `cargo clippy -D warnings` están limpios.
3. Cobertura ≥ 90 % en los módulos tocados (95 % en `core/`).
4. Si el cambio afecta a la API pública, hay docstrings, una entrada en `CHANGELOG.md` y un ejemplo en `docs/`.
5. La casilla de la tarea está marcada en `docs/PLAN.md`, y se ha anotado cualquier desviación del plan.

## Qué no hacer

- Añadir dependencias al núcleo, o añadir cualquier dependencia sin preguntar (`uv add` y `cargo add` piden confirmación).
- Publicar en PyPI o crates.io, crear tags o hacer push: los comandos de publicación están denegados en `.claude/settings.json`.
- Optimizar en Python algo que la fase 7 llevará a Rust. Primero se prioriza que sea correcto y claro.
- Cambiar la semántica de un operador sin actualizar `docs/spec/` y los tests de paridad.

## Herramientas de Claude en este repo (`.claude/`)

- **Skills**:
  - `/plan-task [fase.tarea]` implementa una tarea de `docs/PLAN.md` de principio a fin.
  - `/add-operator <nombre>` añade un nodo al AST en todos los backends.
  - `/bench [suite]` ejecuta benchmarks y los compara con la línea base y con los rivales.
- **Subagentes**:
  - `api-reviewer` revisa los cambios de API pública (solo lectura).
  - `parity-tester` escribe tests diferenciales entre backends.
  - `rust-engineer` trabaja en `crates/`.
