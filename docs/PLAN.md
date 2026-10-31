# Plan de desarrollo (v2: núcleo Rust, bindings multilenguaje)

> Documento vivo y fuente de verdad del roadmap. Cada tarea es una casilla; al completarla se marca y se anotan debajo las desviaciones. Claude lo sigue con `/plan-task`.
>
> v1 (2026-10-30): implementación en Python con Rust como acelerador opcional. **v2 (2026-10-31)**: decisión del autor de poner **Rust en el centro** y ofrecer bindings para **Rust, C/C++, Python, JavaScript, Java/Scala y Go**. La implementación Python de la v1 se **descarta**; su valor se conserva en la especificación y en los vectores de conformidad que se extraen de ella (fase A). El plan v1 completo está en el historial de git.

## 0. Visión

Un motor de reglas con un **único núcleo en Rust** que sirve, con la misma semántica en todos los lenguajes, para:

| Caso de uso | Rival principal | Por dónde ganamos |
|---|---|---|
| Reglas de negocio | Drools, zen-engine, rule-engine | Mismo motor y mismas reglas en 7 lenguajes; tablas de decisión; **auditoría estática**; explicación con requisitos `all`/`any` y contrafactuales |
| Sistema experto | Drools, CLIPS, experta | Rete en Rust, mantenimiento de verdad, hechos con grado, árbol de derivación |
| Validación de datos | pandera, Great Expectations | Mismas reglas que en negocio; evaluación vectorizada sobre Arrow; explicación por fila |
| Explicaciones de ML | imodels, alibi | Reglas extraídas que se ejecutan y auditan en cualquier lenguaje (extracción en Python) |
| Lógica difusa | simpful, scikit-fuzzy, jFuzzyLogic | Normas difusas en el núcleo, disponibles en todos los lenguajes; fuzzytool para lo avanzado en Python |

Lo que nos diferencia es la combinación: **condiciones como AST serializable** + **lógica intercambiable** + **explicación y auditoría** + **un núcleo, siete lenguajes, una batería de conformidad**.

## 1. Decisiones de arquitectura

| # | Decisión | Motivo |
|---|---|---|
| D1 | El **núcleo en Rust** (`crates/core`) es la única implementación de la semántica | Un solo motor, mismo comportamiento en todos los lenguajes |
| D2 | Los bindings son **capas finas**: construcción idiomática de reglas, conversión de datos y ejecución de acciones; ninguna lógica de evaluación | Evita divergencias entre lenguajes |
| D3 | El **contrato entre lenguajes** es el formato JSON del AST, de las reglas, de los datos y de las decisiones y explicaciones (`spec/`), versionado | Una regla escrita en Java se evalúa igual en Go |
| D4 | La semántica la definen la **especificación** y la **batería de conformidad** (`conformance/`), que todos los bindings ejecutan | Sustituye al oráculo Python de la v1 |
| D5 | **Modelo de datos neutro** (`Value`: null, bool, int64, float64, decimal, string, date, datetime, lista, mapa) más **lotes Arrow** para el modo vectorizado | Cruzar la frontera de lenguajes una vez por entrada o por lote, no por campo |
| D6 | Las **acciones** se ejecutan en el lenguaje anfitrión; el núcleo es puro (`evaluate`), y `run` es del binding | Ya validado en la v1; sin callbacks en el camino crítico |
| D7 | **C ABI** estable (`crates/ffi`, cabecera con cbindgen) como base de C/C++, Go y Java (FFM). Bindings dedicados donde aportan: PyO3 (Python), napi-rs y wasm-bindgen (JavaScript) | Un solo punto de FFI para la mayoría de lenguajes |
| D8 | **Lógica difusa**: t-normas, s-normas y complementos en el núcleo Rust (con fuzzytool como referencia de comportamiento); lo avanzado (ANFIS, tipo 2, clustering) sigue en fuzzytool, desde Python | Difusa básica en todos los lenguajes sin duplicar fuzzytool entero |
| D9 | Regex con el crate `regex` (tiempo lineal, sin *lookaround* ni *backreferences*): su semántica es la de referencia | Desaparece el problema de compatibilidad Python/Rust; sin ReDoS |
| D10 | Monorepo: workspace de Cargo + un paquete por lenguaje, CI única | Versiones sincronizadas y conformidad en cada cambio |
| D11 | El proyecto se llama **cleanrules** (decidido el 2026-10-31; *clean* ≈ *Kleene*) | "RulePy" no encaja fuera de Python |

**Nombres por registro** (libres a 2026-10-31 en crates.io, PyPI, npm con y sin scope, y como organización de GitHub; sin marcas ni productos homónimos conocidos). Descartados:

- `rulengine`: ocupado en PyPI por un paquete abandonado y confundible con `rule-engine`;
- `rulewise`: RuleWise® es marca de un framework de cumplimiento normativo, y la organización de GitHub está ocupada;
- `rulewizard`: RULEWIZARD es marca registrada de software (EE. UU., nº 2652799) y producto de Parasoft;
- `rulemachine`: "Rules Machine" ya es un motor de reglas serializables en npm, y "Rule Machine" es la app de reglas de Hubitat.

| Registro | Nombre |
|---|---|
| crates.io | `cleanrules` (núcleo, en `crates/core`), `cleanrules-ffi`, `cleanrules-cli` |
| PyPI | `cleanrules` (`import cleanrules`) |
| npm | `cleanrules`; binarios por plataforma de napi-rs en el scope `@cleanrules/*` |
| Maven Central | `io.github.cleanrules:cleanrules` (+ `cleanrules-scala_3`) |
| Go | `github.com/cleanrules/cleanrules/bindings/go` |
| C/C++ | biblioteca `libcleanrules`, prefijo de símbolos `cr_`, paquetes Conan y vcpkg `cleanrules` |

### Decisiones pendientes (confirmar con el autor)

- [x] **Nombre del proyecto**: `cleanrules`. La implementación Python de la v1 conserva `rulepy` hasta su retirada en la fase A.
- [ ] Java: FFM (Java 22+, sin código nativo propio) frente a JNI (Java 8+). Propuesto: FFM, con JNI solo si hace falta soportar Java antiguo.
- [ ] JavaScript: napi-rs (Node, más rápido) y wasm (navegador, Deno). Propuesto: los dos, con la misma API TypeScript.
- [ ] MSRV de Rust y edición (propuesto: edición 2024, MSRV = estable actual menos 2).
- [ ] UNKNOWN en lógica difusa como intervalo `[0, 1]` (propuesta de la v1, ver `spec/ast.md`).
- [x] Organización de GitHub: `cleanrules` (la crea el autor el 2026-10-31). La necesitan el groupId de Maven y la ruta del módulo Go.

## 2. Estructura objetivo del repositorio

```
<repo>/
├── CLAUDE.md · .claude/            instrucciones y herramientas de Claude
├── Cargo.toml                      workspace de Rust
├── crates/
│   ├── core/                       AST, valores, evaluador, lógicas, reglas, políticas,
│   │                               explain, summary, (después) Rete, auditoría, Arrow
│   ├── ffi/                        C ABI: handles opacos, errores, JSON in/out; cbindgen
│   └── cli/                        (opcional) evaluar reglas JSON sobre datos JSON
├── bindings/
│   ├── python/                     PyO3 + maturin (paquete `cleanrules`)
│   ├── c/ · cpp/                   cabecera C generada; envoltorio C++ RAII; CMake
│   ├── js/                         napi-rs (Node) + wasm-bindgen (web); TypeScript
│   ├── jvm/                        Java (FFM sobre la C ABI) + módulo Scala
│   └── go/                         cgo sobre la C ABI
├── spec/                           ast.md, rules.md, data-model.md, json-format.md, *.schema.json
├── conformance/                    vectores JSON + un runner por lenguaje
├── benchmarks/
└── docs/                           PLAN.md, sitio de documentación
```

## 3. Fases

Cada fase termina en algo publicable o verificable. Tamaños relativos (S/M/L/XL), no estimaciones de tiempo.

---

### Fase P. Implementación Python de referencia (v1) — ✅ completada, se retira en la fase A

Se hizo en Python, con 676 tests y un 100 % de cobertura, todo lo que hoy define la semántica:
- AST, `F` y trampas de Python (`docs/pitfalls.md`);
- lógica de Kleene, evaluador con motivos estructurados y lectura segura de datos;
- `Rule`, `RuleSet` y políticas DMN, `Decision`, acciones con `run`;
- `explain()` con requisitos `all`/`any`, contrafactuales y validación de opciones;
- `summary()`, `field_paths()`, `Decimal` exacto.

El historial está en git hasta el commit que cierra la 2.12 (y un tag `python-reference`). Las lecciones de sus revisiones (seguridad de la lectura de atributos, igualdad en `ANY`, agregados con aritmética normalizada, salidas inmutables, explicaciones con conflictos) son requisitos de la reimplementación y están en `spec/`.

---

### Fase 0'. Adaptar las especificaciones para Claude · tamaño S

- [x] 0'.1 Reescribir `CLAUDE.md` para la nueva arquitectura (invariantes, comandos, estructura).
  - Se añadieron dos invariantes: memoria segura (sin `unsafe` en el núcleo) y dependencias mínimas con *features*. "Python es el oráculo" pasa a "la especificación y la conformidad son el oráculo".
- [ ] 0'.2 `.claude/`: `rust-engineer` pasa a ser el agente principal; `parity-tester` pasa a `conformance-tester`; `api-reviewer` revisa la API en todos los lenguajes; `/add-operator` recorre núcleo, especificación, vectores y bindings; permisos para cargo, maturin, npm, gradle/sbt y go (instalar dependencias pide confirmación).
  - Hechos los agentes y las skills. Pendientes de que el autor los aplique (Claude no puede modificar sus propios permisos ni hooks): los permisos de `.claude/settings.json` y el hook `format.sh`, que añade gofmt, clang-format y prettier.

---

### Fase A. Contrato entre lenguajes y retirada de Python · tamaño M

**Objetivo**: que la especificación y los vectores contengan todo el conocimiento de la v1 antes de borrar el código.

- [ ] A.1 Mover `docs/spec/` a `spec/` y escribir `spec/json-format.md` y los JSON Schemas (antes tarea 3.1): AST con `"version": 1`, reglas y rule sets, valores (`Decimal` como texto, `inf`/`-inf`, fechas ISO 8601, zonas horarias), decisiones, motivos (`Reason`), explicaciones (requisitos, `Counterfactual`) y resúmenes.
- [ ] A.2 `spec/data-model.md`: `Value` y cómo convierte cada binding sus tipos nativos. La sección "Reading data" de `ast.md` (mapas, registros, atributos **públicos**, nunca privados ni de módulos o clases) pasa a ser una obligación de cada binding.
- [ ] A.3 Exportador temporal en Python (`conformance/tools/`) que, con la implementación de la v1, genera **vectores de conformidad**: árboles y errores de construcción, evaluaciones con motivos, modo estricto, decisiones de las 6 políticas, agregados, explicaciones y resúmenes. Las propiedades de hypothesis se convierten en miles de casos fijados.
- [ ] A.4 Retirada: tag `python-reference` y eliminación de `src/rulepy` y de sus tests. Se conservan `spec/`, `docs/pitfalls.md` (pasa a la documentación del binding de Python) y los vectores.

**Aceptación**: el conjunto de vectores cubre cada sección de la especificación; un script comprueba que cada regla normativa de `spec/` cita al menos un vector.

---

### Fase B. Núcleo Rust · tamaño XL · v0.1 (crate)

**Objetivo**: toda la funcionalidad de la fase P, en Rust, superando los vectores de conformidad.

- [ ] B.1 Andamiaje: workspace, edición y MSRV, `clippy` pedante, `rustfmt`, `cargo-deny`, `cargo-nextest`, CI (Linux, macOS, Windows), `miri` en las partes con `unsafe` (idealmente ninguna en `core`).
- [ ] B.2 Modelo de valores (`Value`, `Decimal` exacto con `rust_decimal`, fechas con `jiff` o `chrono`), normalización y grupos de tipo.
- [ ] B.3 AST con serde: los 14 tipos de nodo y sus validaciones al construir, identidad estructural, `field_paths`, `Display` legible.
- [ ] B.4 API de construcción idiomática en Rust (constructores y sobrecarga de operadores donde tenga sentido; macros solo si aportan).
- [ ] B.5 Lógicas: `Logic` (trait), Kleene, registro por nombre y capacidad `Graded`.
- [ ] B.6 Evaluador: lectura de `Value` por rutas, nulos, tipos, aritmética (float64, desbordamiento de 64 bits, `Decimal`), todos los nodos, árbol de resultados con `Reason`, modo estricto y orden de errores.
- [ ] B.7 Reglas, rule sets, políticas DMN, payloads, agregados y `Decision`.
- [ ] B.8 `explain` (requisitos `all`/`any`, De Morgan, fusión y absorción, validación de opciones con apariciones vinculadas, contrafactuales) y `summary`.
- [ ] B.9 Runner de conformidad en Rust y tests de propiedades con `proptest` (leyes lógicas, De Morgan, suficiencia de las explicaciones).
- [ ] B.10 Benchmarks con `criterion` (evaluación por lote, explicación), comparados con zen-engine.

**Aceptación**: el 100 % de los vectores en verde; `clippy -D warnings`; sin `unsafe` en `core`.

---

### Fase C. Binding Python · tamaño L · primera versión pública

- [ ] C.1 PyO3 + maturin, wheels `abi3` y build *free-threaded*, CI de wheels.
- [ ] C.2 API Python con la ergonomía de la v1: `F`, operadores, protección contra trampas (`docs/pitfalls.md`), `Rule`, `RuleSet`, `Decision`, `explain`, `summary`.
- [ ] C.3 Conversión de datos (dicts, dataclasses, Pydantic, namedtuples, filas de pandas, numpy) a `Value`, con las reglas de seguridad de `spec/data-model.md`.
- [ ] C.4 `run` y acciones en Python; pickle; tipos (`.pyi`).
- [ ] C.5 Runner de conformidad en Python.

---

### Fase D. Reglas como datos · tamaño M

- [ ] D.1 Carga y volcado JSON y YAML en el núcleo (todos los lenguajes la heredan).
- [ ] D.2 DSL de texto (`IF ... THEN ...`, sintaxis alineada con `fuzzytool.rule_from_text`) con parser propio en Rust y errores con línea y columna.
- [ ] D.3 Tablas de decisión con políticas DMN (la prioridad de los valores de salida se compila a prioridades de regla), desde filas, CSV o JSON.
- [ ] D.4 Acciones por nombre registrado en cada binding (nunca rutas de importación leídas de un fichero).

---

### Fase E. C ABI y bindings C/C++ · tamaño L

- [ ] E.1 `crates/ffi`: handles opacos, propiedad de la memoria explícita, errores como códigos más mensaje, entrada y salida en JSON y en `Value` binario; cabecera con cbindgen; versionado de ABI.
- [ ] E.2 Cabecera C++ RAII (`unique_ptr` con deleters, excepciones opcionales), paquete CMake, Conan y vcpkg.
- [ ] E.3 Runner de conformidad en C++ y tests con sanitizers (ASan, UBSan).

---

### Fase F. JavaScript · tamaño L

- [ ] F.1 napi-rs para Node (binarios precompilados por plataforma) y wasm-bindgen para navegador y Deno.
- [ ] F.2 API TypeScript idiomática común a ambos (el proxy `F` se puede expresar con `Proxy`; `Decimal` como texto o bigint).
- [ ] F.3 Runner de conformidad y publicación en npm.

---

### Fase G. JVM: Java y Scala · tamaño L

- [ ] G.1 Java sobre la C ABI con FFM (o JNI, según la decisión pendiente); `BigDecimal`, `java.time`; artefactos nativos por plataforma dentro del jar.
- [ ] G.2 Capa Scala idiomática (operadores, case classes).
- [ ] G.3 Runner de conformidad; publicación en Maven Central.

---

### Fase H. Go · tamaño M

- [ ] H.1 cgo sobre la C ABI, bibliotecas estáticas precompiladas por plataforma, API idiomática (`error` como valor).
- [ ] H.2 Runner de conformidad; módulo Go publicado.

---

### Fase I. Validación de datos vectorizada · tamaño L

- [ ] I.1 Evaluación sobre lotes Arrow en el núcleo (en paralelo con rayon), con los mismos nulos, tipos y motivos que el evaluador por filas.
- [ ] I.2 Severidad, reglas de conjunto de datos (unicidad, claves foráneas, agregados) e informe de validación con explicación por fila.
- [ ] I.3 Integración por lenguaje: polars y pandas (Python), apache-arrow (JS), Arrow Java.

---

### Fase J. Lógica difusa y auditoría estática · tamaño L

- [ ] J.1 Lógica difusa en el núcleo: t-normas, s-normas y complementos de fuzzytool (`min`, `prod`, `lukasiewicz`, `einstein`, `hamacher`, `drastic`, `nilpotent`, familias Yager, Dombi y Frank), con tests contra fuzzytool como referencia; UNKNOWN como intervalo si se confirma.
- [ ] J.2 Grados de disparo, umbral α, políticas `MAX_DEGREE` y `WEIGHTED`, comparaciones suaves, explicaciones con grados.
- [ ] J.3 En Python: interoperabilidad con fuzzytool (términos como condiciones, sistemas Mamdani/TSK como acciones); el cambio de `Antecedent.__and__` en fuzzytool 0.8.
- [ ] J.4 Auditoría estática en el núcleo: intervalos por campo, condiciones insatisfacibles, reglas ensombrecidas, solapamientos, contradicciones y huecos; z3 opcional.

---

### Fase K. Sistema experto · tamaño XL

- [ ] K.1 Hechos tipados y memoria de trabajo; patrones con variables y joins; negación.
- [ ] K.2 Rete incremental en Rust; agenda con estrategias deterministas; mantenimiento de verdad.
- [ ] K.3 Acciones declarativas (`assert`, `modify`, `retract`, `Set`, `Emit`) ejecutadas en el núcleo; acciones del anfitrión como ruta lenta documentada.
- [ ] K.4 Explicación por árbol de derivación; encadenamiento hacia atrás básico; benchmarks Miss Manners y Waltz contra Drools y CLIPS.

---

### Fase L. Explicaciones de ML (binding Python) · tamaño L

- [ ] L.1 Extracción de reglas desde scikit-learn, árbol sustituto, RuleFit (imodels) y extracción difusa con fuzzytool; las reglas se exportan en JSON y se ejecutan en cualquier lenguaje.
- [ ] L.2 Métricas (fidelidad, cobertura, complejidad) y `Guarded(estimator, ruleset)` compatible con scikit-learn.

---

### Fase M. Documentación y publicación 1.0 · tamaño M

- [ ] M.0 Documentación con Zensical (Material for MkDocs termina su soporte el 5 de mayo de 2027).
- [ ] M.1 Guías por lenguaje con los mismos ejemplos, referencia de la especificación, matriz de funcionalidades y benchmarks reproducibles.
- [ ] M.2 Publicación sincronizada (crates.io, PyPI, npm, Maven Central, módulo Go, Conan y vcpkg) con *trusted publishing* donde exista; política de estabilidad de la API y de la ABI.
- [ ] M.3 Paper en JOSS, DOI en Zenodo y `CITATION.cff`.

## 4. Prácticas transversales

| Qué | Cómo |
|---|---|
| Semántica | `spec/` primero; cualquier cambio de comportamiento actualiza la especificación y los vectores en el mismo PR |
| Conformidad | Todos los bindings ejecutan `conformance/` en CI; un binding no se publica con vectores en rojo |
| Propiedades | `proptest` en el núcleo; los contraejemplos encontrados se añaden como vectores |
| Seguridad | Sin `unsafe` en `core`; `unsafe` en `ffi` con comentario `// SAFETY:` y `miri`; regex en tiempo lineal; límites de profundidad y tamaño al parsear; lectura de datos solo de atributos públicos |
| Rendimiento | Nadie afirma una mejora sin benchmark (`/bench`), con mediana y rango intercuartílico |
| Versionado | SemVer común a todos los paquetes; la versión del formato JSON es independiente |

## 5. Dependencias entre fases

```
P (hecha) → 0' → A → B → C ─┬→ D → E → F → G → H
                            ├→ I (necesita B; integra con C, F, G)
                            ├→ J (necesita B y D)
                            ├→ K (necesita B y D)
                            └→ L (necesita C)
todas → M
```

Los bindings E–H siguen el orden decidido (C/C++ → JS → JVM → Go). Las fases de funcionalidad I–K se implementan en el núcleo y llegan a todos los bindings existentes.

## 6. Riesgos

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Siete ecosistemas que empaquetar y mantener | Alto | Bindings finos, C ABI común, conformidad compartida y CI única desde E.1 |
| Divergencias entre lenguajes | Alto | Sin lógica en los bindings; vectores obligatorios |
| Perder conocimiento al retirar Python | Medio | Fase A antes de borrar; tag en git |
| Fallos de memoria en la frontera FFI | Alto | Handles opacos, propiedad explícita, sanitizers y `miri` |
| Coste de convertir datos entre lenguajes | Medio | `Value` y lotes Arrow; benchmarks por binding |
| Lógica difusa duplicada respecto a fuzzytool | Bajo | Solo las normas; tests contra fuzzytool como referencia |
