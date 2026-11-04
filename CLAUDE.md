# cleanrules

Motor de reglas con **un único núcleo en Rust** y bindings para Rust, C/C++, Python, JavaScript, Java/Scala, Go y Erlang. Cubre cuatro casos de uso con la misma semántica en todos los lenguajes: reglas de negocio, sistema experto (encadenamiento hacia delante), validación de datos y explicaciones de modelos de ML. Además, los agentes de IA lo usan como herramienta de decisión y como *guardrail* explicable, por un servidor MCP y adaptadores para sus frameworks. Admite lógica nítida (Kleene, tres valores) y difusa (normas de `fuzzytool` portadas al núcleo).

- **Hoja de ruta y estado**: [docs/PLAN.md](docs/PLAN.md) es la fuente de verdad. Antes de empezar cualquier tarea, léela y localiza la fase y la tarea.
- **Contratos**: la especificación ([spec/](spec/)) y la batería de conformidad (`conformance/`) definen la semántica. Si cambia el comportamiento, primero se actualizan la especificación y los vectores.
- **Fase actual**: A, B, C, D y E completadas (especificación, formato JSON v1 y vectores de conformidad; núcleo Rust en `crates/core`, con CI que exige el 95 % de cobertura de líneas; binding de Python en `bindings/python`, que pasa todos los vectores por JSON y con datos nativos; reglas como datos: YAML, texto, tablas de decisión y acciones por nombre; C ABI en `crates/ffi` y C++ en `bindings/cpp`, con CMake, Conan y vcpkg). Siguiente: fase F (JavaScript); O.1–O.3 y V.1–V.2 ya pueden empezar.

## Estructura

```text
crates/core      el motor: valores, AST, lógicas, evaluador, reglas, políticas, explain, summary (crate `cleanrules`)
crates/ffi       C ABI (cbindgen) que usan C/C++, Go y la JVM (crate `cleanrules-ffi`)
crates/mcp       servidor MCP para agentes de IA (crate `cleanrules-mcp`, fase O)
bindings/<lang>  python (PyO3), c, cpp, js (napi-rs + wasm), jvm (Java FFM + Scala), go (cgo), erlang (rustler)
spec/            especificación normativa y JSON Schemas
conformance/     vectores JSON, su comprobador (tools/check.py) y un runner por lenguaje
scripts/         utilidades del repositorio (progress.py: barra de progreso del plan)
benchmarks/      resultados de los benchmarks y comparación con rivales (zen/, fuera del workspace)
docs/            PLAN.md y pitfalls.md (pasa a la documentación del binding de Python en la fase C)
```

Las carpetas aparecen a medida que avanzan las fases; no existe ninguna antes de su tarea.

## Comandos

```bash
# Núcleo Rust (fase B+)
cargo build --workspace
cargo nextest run --workspace        # o `cargo test --workspace`
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo llvm-cov --workspace           # cobertura
cargo bench -p cleanrules              # criterion

# Conformidad (fase A+)
cargo test -p cleanrules --test conformance

# Bindings, cuando existan
(cd bindings/python && uv run pytest && uv run pyright)   # uv compila la extensión con maturin
cargo test -p cleanrules-ffi                                # C ABI: sus tests y los vectores a través de ella
cargo +nightly miri test -p cleanrules-ffi --lib --test abi --test values --test run
(cd crates/ffi && cbindgen --config cbindgen.toml --output include/cleanrules.h)   # regenera la cabecera C
cmake -S bindings/cpp -B build/cpp && ctest --test-dir build/cpp
npm --prefix bindings/js test
(cd bindings/jvm && ./gradlew test)
(cd bindings/go && go test ./...)
(cd bindings/erlang && rebar3 ct)
cargo run -p cleanrules-mcp -- --rules <dir>   # servidor MCP (fase O)

# Especificación y vectores
uv run --no-project --with jsonschema python spec/schemas/check.py
uv run --no-project --with jsonschema python conformance/tools/check.py
uvx ruff check . && uvx ruff format --check .   # herramientas en Python (ruff.toml)
python3 scripts/progress.py                     # progreso del plan
```

Un hook (`.claude/hooks/format.sh`) formatea tras cada edición los `.rs` (rustfmt), `.py` (ruff), `.go` (gofmt) y, si hay herramienta instalada, C/C++ (clang-format) y JS/TS (prettier del proyecto), así que no hace falta formatear a mano.

## Invariantes de arquitectura (no romper)

1. **Las condiciones son datos.** Una condición es un AST serializable, nunca una función opaca. Las *acciones* sí son código del lenguaje anfitrión. Serializar, explicar, vectorizar, auditar y cruzar la frontera entre lenguajes depende de esto.
2. **Un solo motor.** Toda la semántica vive en `crates/core`. Los bindings son capas finas: construyen reglas de forma idiomática, convierten datos a `Value`, ejecutan acciones y traducen errores. Nunca evalúan, explican ni deciden por su cuenta.
3. **Cada nodo del AST funciona en todo el núcleo** (evaluador, explicación, resumen, serialización, evaluación Arrow, auditoría y Rete) y se puede construir desde todos los bindings, o devuelve un error `Unsupported` explícito. Para añadir un operador, usa la skill `/add-operator`.
4. **La lógica es intercambiable.** El evaluador nunca combina valores de verdad con `&&`, `||` o `!`: todo pasa por el trait `Logic` (Kleene de tres valores; normas difusas con capacidad `Graded`).
5. **La especificación y la conformidad son el oráculo.** Los vectores de `conformance/` se ejecutan en el núcleo y en todos los bindings. Un cambio de semántica actualiza `spec/`, los vectores y el núcleo en el mismo PR. Los contraejemplos de `proptest` se añaden como vectores.
6. **El formato JSON está versionado** (`"version": <n>` en `spec/json-format.md` y los JSON Schemas). Es el contrato entre lenguajes y con los ficheros de los usuarios; un cambio no aditivo sube la versión.
7. **Determinismo.** Las mismas entradas producen la misma decisión, el mismo orden de disparo y la misma explicación en todos los lenguajes. Nada depende del orden de iteración de un `HashMap`/`HashSet` ni de direcciones de memoria: usa `BTreeMap`, `IndexMap` o vectores ordenados.
8. **Todo resultado se explica.** Cada motor devuelve un resultado con `explain()`. Las explicaciones forman parte de la API pública, tienen forma JSON en la especificación y tests de *snapshot*.
9. **Nada de `eval`.** El DSL de texto tiene parser propio, YAML se carga de forma segura y las regex usan el crate `regex` (tiempo lineal, sin *lookaround* ni *backreferences*). Los parsers tienen límites de profundidad y tamaño.
10. **Memoria segura.** Sin `unsafe` en `crates/core`. En `crates/ffi` y los bindings, cada bloque `unsafe` lleva un comentario `// SAFETY:` y se prueba con `miri` o sanitizers. La propiedad de cada puntero de la C ABI está documentada en la cabecera.
11. **Dependencias mínimas.** El núcleo depende solo de lo imprescindible (serde, regex, decimal, fechas). Arrow, YAML, rayon y z3 van detrás de *features* de Cargo. En cada binding, las integraciones (pandas, polars, fuzzytool, scikit-learn…) son opcionales y se cargan de forma perezosa. Lo mismo vale para los frameworks de agentes, y el SDK de MCP vive solo en `crates/mcp`.
12. **La IA usa el motor, no forma parte de él.** Ni el núcleo ni los bindings llaman a modelos de lenguaje. Los agentes llegan por MCP o por adaptadores finos, que no ejecutan acciones ni leen fuera de lo configurado. Una regla propuesta por un LLM es un dato que se valida, se audita y se prueba como cualquier otro.

## Coherencia con fuzzytool

fuzzytool es del mismo autor, y cleanrules debe sentirse como su hermana en todos los lenguajes:

- operadores `&` (AND/t-norma), `|` (OR/s-norma) y `~` (NOT), o su equivalente idiomático donde no hay sobrecarga;
- conectivas y estrategias resueltas por nombre mediante registros;
- verbos `explain()`, `audit()`, `summary()` y `rule_from_text()`.

Las t-normas, s-normas y complementos se portan al núcleo con **fuzzytool como referencia de comportamiento** (tests contra sus valores). Lo avanzado (ANFIS, tipo 2, clustering) no se reimplementa: se usa fuzzytool desde el binding de Python.

Un detalle para el binding de Python: `fuzzytool.sets.Antecedent.__and__` no devuelve `NotImplemented` cuando recibe un tipo ajeno, así que `score["poor"] & (F.x > 2)` construye un `And` de fuzzytool inválido. Hay que poner la expresión de cleanrules a la izquierda o envolver el término. La corrección en fuzzytool está prevista en la fase J.3.

## Convenciones

- **Todo lo que se publica va en inglés**: código, identificadores, documentación, mensajes de error y commits. Este fichero y `docs/PLAN.md` están en español.
- **Rust**: edición 2024, `clippy` pedante, `rustfmt`. Errores con `thiserror`: un tipo de error del núcleo con códigos estables (los mismos que en la especificación) y mensajes que indican qué regla o nodo falla y cómo corregirlo. Sin `unwrap`/`expect` en código de biblioteca salvo invariantes demostrables y comentadas. Logging con `tracing`, nunca `println!`.
- **Bindings**: API idiomática de cada lenguaje (excepciones en Python, Java y C++; `error` como valor en Go; códigos de retorno en C; tuplas `{ok, _}` y `{error, _}` en Erlang), con los mismos nombres de conceptos (`Rule`, `RuleSet`, `Decision`, `explain`, `summary`) y la misma jerarquía de errores, que en Python cuelga de `cleanrules.CleanRulesError`.
- **API pública**: lo exportado por cada paquete. Cualquier cambio actualiza `CHANGELOG.md` (Keep a Changelog), la documentación de los lenguajes afectados y, si toca el formato, la especificación. SemVer común a todos los paquetes; la versión del formato JSON es independiente.
- **Commits**: Conventional Commits con ámbito por componente: `core`, `ffi`, `py`, `c`, `cpp`, `js`, `jvm`, `go`, `erl`, `mcp`, `agents`, `spec`, `conformance`, `docs`, `ci`. Un commit o PR por tarea del plan.
- **Tests**: unitarios junto al código en Rust; propiedades con `proptest` (leyes lógicas, De Morgan, suficiencia de explicaciones); vectores en `conformance/` para todo lo observable desde fuera; *snapshots* con `insta` para texto de `explain()` y `summary()`. Los tests lentos se marcan (`#[ignore]` con motivo, o `slow` en cada binding).

## Definición de terminado (cada tarea)

1. Los tests nuevos fallaban antes del cambio y ahora pasan, y pasa la suite completa.
2. `cargo clippy -D warnings` y `cargo fmt --check` limpios, y los linters de cada binding tocado (ruff y pyright en Python, eslint/tsc en JS, `go vet` en Go).
3. Los vectores de conformidad pasan en el núcleo y en todos los bindings existentes.
4. Cobertura ≥ 90 % en lo tocado (95 % en `crates/core`).
5. Si el cambio afecta a la API pública, hay documentación, una entrada en `CHANGELOG.md` y un ejemplo en `docs/`.
6. La casilla de la tarea está marcada en `docs/PLAN.md`, y se ha anotado cualquier desviación del plan.

## Qué no hacer

- Añadir dependencias sin preguntar (`cargo add`, `uv add`, `npm install <paquete>`, `go get` y Gradle piden confirmación). En el núcleo, solo con un motivo fuerte.
- Publicar en cualquier registro, crear tags o hacer push: los comandos de publicación están denegados en `.claude/settings.json`.
- Poner lógica de evaluación en un binding, o "arreglar" en un binding una diferencia que debe corregirse en el núcleo.
- Cambiar la semántica sin actualizar `spec/` y los vectores.
- Afirmar una mejora de rendimiento sin benchmark (`/bench`).

## Herramientas de Claude en este repo (`.claude/`)

- **Skills**:
  - `/plan-task [fase.tarea]` implementa una tarea de `docs/PLAN.md` de principio a fin.
  - `/add-operator <nombre>` añade un nodo al AST en la especificación, los vectores, el núcleo y los bindings.
  - `/bench [suite]` ejecuta benchmarks y los compara con la línea base y con los rivales.
- **Subagentes**:
  - `rust-engineer` es el agente principal: trabaja en `crates/` y en la parte Rust de los bindings (PyO3, napi-rs, wasm, C ABI).
  - `conformance-tester` escribe vectores de conformidad y propiedades, y ejecuta los runners de todos los lenguajes.
  - `api-reviewer` revisa los cambios de API pública en todos los lenguajes y en el formato JSON (solo lectura).
