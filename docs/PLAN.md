# Plan de desarrollo (v3: núcleo Rust, bindings multilenguaje, agentes de IA)

> Documento vivo y fuente de verdad del roadmap. Cada tarea es una casilla; al completarla se marca y se anotan debajo las desviaciones. Claude lo sigue con `/plan-task`.
>
> v1 (2026-10-30): implementación en Python con Rust como acelerador opcional. **v2 (2026-10-31)**: decisión del autor de poner **Rust en el centro** y ofrecer bindings para **Rust, C/C++, Python, JavaScript, Java/Scala y Go**. La implementación Python de la v1 se **descarta**; su valor se conserva en la especificación y en los vectores de conformidad que se extraen de ella (fase A). El plan v1 completo está en el historial de git.
>
> **v3 (2026-11-01)**: decisión del autor de añadir **Erlang** (fase N) y la **conexión con IA agéntica y frameworks de agentes** (fase O).

## 0. Visión

Un motor de reglas con un **único núcleo en Rust** que sirve, con la misma semántica en todos los lenguajes, para:

| Caso de uso | Rival principal | Por dónde ganamos |
|---|---|---|
| Reglas de negocio | Drools, zen-engine, rule-engine | Mismo motor y mismas reglas en 8 lenguajes; tablas de decisión; **auditoría estática**; explicación con requisitos `all`/`any` y contrafactuales |
| Sistema experto | Drools, CLIPS, experta | Rete en Rust, mantenimiento de verdad, hechos con grado, árbol de derivación |
| Validación de datos | pandera, Great Expectations | Mismas reglas que en negocio; evaluación vectorizada sobre Arrow; explicación por fila |
| Explicaciones de ML | imodels, alibi | Reglas extraídas que se ejecutan y auditan en cualquier lenguaje (extracción en Python) |
| Lógica difusa | simpful, scikit-fuzzy, jFuzzyLogic | Normas difusas en el núcleo, disponibles en todos los lenguajes; fuzzytool para lo avanzado en Python |
| IA agéntica | OPA/Rego, Cedar, NeMo Guardrails, Guardrails AI | Decisiones deterministas y explicadas como herramientas y *guardrails* de agentes (MCP y adaptadores por framework); reglas que un LLM propone como datos y el motor valida, audita y prueba antes de usarlas; contrafactuales que dicen al agente qué cambiar |

Lo que nos diferencia es la combinación: **condiciones como AST serializable** + **lógica intercambiable** + **explicación y auditoría** + **un núcleo, ocho lenguajes, una batería de conformidad** + **un puente estable con los agentes de IA**.

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
| D12 | **Erlang** con un NIF en Rust hecho con `rustler` directamente sobre `crates/core` (no sobre la C ABI); los rule sets son recursos inmutables que comparten los procesos, y el trabajo largo va a *dirty schedulers* | rustler convierte términos y captura *panics*, y es el estándar de la BEAM; un NIF que bloquea o falla tumbaría la VM |
| D13 | **La IA usa el motor, no forma parte de él**: ni el núcleo ni los bindings llaman a modelos de lenguaje. Los agentes llegan por un servidor MCP (`crates/mcp`) y por adaptadores finos y opcionales para cada framework | Determinismo y dependencias mínimas; MCP sirve a la mayoría de frameworks con una sola implementación |
| D14 | **Visualización como texto**: el núcleo escribe diagramas en DOT, Mermaid y HTML autónomo, sin dependencias; ninguna biblioteca gráfica entra en el núcleo, y en los bindings dibujar o mostrar es un extra opcional de carga perezosa | El mismo diagrama en todos los lenguajes, probado con *snapshots*; dependencias mínimas |

**Nombres por registro** (libres a 2026-10-31 en crates.io, PyPI, npm con y sin scope, y como organización de GitHub; sin marcas ni productos homónimos conocidos). Descartados:

- `rulengine`: ocupado en PyPI por un paquete abandonado y confundible con `rule-engine`;
- `rulewise`: RuleWise® es marca de un framework de cumplimiento normativo, y la organización de GitHub está ocupada;
- `rulewizard`: RULEWIZARD es marca registrada de software (EE. UU., nº 2652799) y producto de Parasoft;
- `rulemachine`: "Rules Machine" ya es un motor de reglas serializables en npm, y "Rule Machine" es la app de reglas de Hubitat.

| Registro | Nombre |
|---|---|
| crates.io | `cleanrules` (núcleo, en `crates/core`), `cleanrules-ffi`, `cleanrules-cli`, `cleanrules-mcp` (servidor MCP; libre a 2026-11-01) |
| PyPI | `cleanrules` (`import cleanrules`) |
| npm | `cleanrules`; binarios por plataforma de napi-rs en el scope `@cleanrules/*` |
| Maven Central | `io.github.cleanrules:cleanrules` (+ `cleanrules-scala_3`) |
| Go | `github.com/cleanrules/cleanrules/bindings/go` |
| C/C++ | biblioteca `libcleanrules`, prefijo de símbolos `cr_`, paquetes Conan y vcpkg `cleanrules` |
| Hex (Erlang) | `cleanrules`: aplicación OTP y módulo `cleanrules` (libre a 2026-11-01) |

### Decisiones pendientes (confirmar con el autor)

- [x] **Nombre del proyecto**: `cleanrules`. La implementación Python de la v1 conserva `rulepy` hasta su retirada en la fase A.
- [ ] Java: FFM (Java 22+, sin código nativo propio) frente a JNI (Java 8+). Propuesto: FFM, con JNI solo si hace falta soportar Java antiguo.
- [ ] JavaScript: napi-rs (Node, más rápido) y wasm (navegador, Deno). Propuesto: los dos, con la misma API TypeScript.
- [x] MSRV de Rust y edición: edición 2024 y MSRV 1.95, la estable menos dos (decidido el 2026-10-31).
- [ ] UNKNOWN en lógica difusa como intervalo `[0, 1]` (propuesta de la v1, ver `spec/ast.md`).
- [ ] Erlang: cómo se distribuye el NIF (binarios precompilados por plataforma frente a compilarlo con Rust al instalar) y si hay además una capa Elixir. Propuesto: precompilados; Elixir y Gleam usan el módulo Erlang directamente, y una capa propia solo si se pide.
- [ ] Agentes: SDK de MCP en Rust (propuesto: `rmcp`, el oficial) y frameworks con adaptador propio. Propuesto: Claude Agent SDK, OpenAI Agents SDK, LangChain/LangGraph y Pydantic AI en Python; Vercel AI SDK en JavaScript; LangChain4j y Spring AI en la JVM; los demás, por MCP.
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
│   ├── mcp/                        servidor MCP para agentes de IA (`cleanrules-mcp`)
│   └── cli/                        (opcional) evaluar reglas JSON sobre datos JSON
├── bindings/
│   ├── python/                     PyO3 + maturin (paquete `cleanrules`)
│   ├── c/ · cpp/                   cabecera C generada; envoltorio C++ RAII; CMake
│   ├── js/                         napi-rs (Node) + wasm-bindgen (web); TypeScript
│   ├── jvm/                        Java (FFM sobre la C ABI) + módulo Scala
│   ├── go/                         cgo sobre la C ABI
│   └── erlang/                     NIF con rustler, aplicación OTP y rebar3
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
- [x] 0'.2 `.claude/`: `rust-engineer` pasa a ser el agente principal; `parity-tester` pasa a `conformance-tester`; `api-reviewer` revisa la API en todos los lenguajes; `/add-operator` recorre núcleo, especificación, vectores y bindings; permisos para cargo, maturin, npm, gradle/sbt y go (instalar dependencias pide confirmación).
  - Los permisos de `.claude/settings.json` y el hook `format.sh` (que añade gofmt, clang-format y prettier) los aplicó el autor el 2026-10-31, porque Claude no puede modificar sus propios permisos ni hooks.

---

### Fase A. Contrato entre lenguajes y retirada de Python · tamaño M

**Objetivo**: que la especificación y los vectores contengan todo el conocimiento de la v1 antes de borrar el código.

- [x] A.1 Mover `docs/spec/` a `spec/` y escribir `spec/json-format.md` y los JSON Schemas (antes tarea 3.1): AST con `"version": 1`, reglas y rule sets, valores (`Decimal` como texto, `inf`/`-inf`, fechas ISO 8601, zonas horarias), decisiones, motivos (`Reason`), explicaciones (requisitos, `Counterfactual`) y resúmenes.
  - La v1 no serializaba nada: el formato se diseña aquí, a partir de sus estructuras. Se divide en:
  - [x] A.1a Mover `docs/spec/` a `spec/`; `spec/json-format.md` con el documento (`"version": 1`), los valores y las condiciones, y sus JSON Schemas.
    - Hecho el 2026-10-31 y revisado por `api-reviewer`. Decisiones tomadas:
      - Valores: JSON sin envolver donde no hay ambigüedad, y objetos `$int`, `$float`, `$decimal`, `$date`, `$datetime` y `$map` en el resto.
      - Los enteros de más de 2⁵³ se escriben como `{"$int": "<dígitos>"}`, por JavaScript y Go.
      - Los decimales tienen texto canónico en notación plana y conservan la escala.
      - Fechas con hora con desfase fijo (`±HH:MM[:SS]`, hasta ±18:00) o con zona IANA (RFC 9557).
      - Las claves duplicadas o desconocidas son error.
      - `format_error` cubre la forma y `expression_error` las reglas de construcción, con un orden de errores fijado.
      - Límites: 16 MiB, anidamiento JSON 1024 y profundidad de nodos 256.
      - Los documentos se comparan como árboles tipados, no como texto.
      - `Const(None)` solo se admite bajo `is_null`; la v1 lo aceptaba bajo `and`, `or` y `not`, y se considera un fallo de la v1.
      - Las regex usan la sintaxis del crate `regex` con sus opciones por defecto (D9).
      - Los esquemas están en `spec/schemas/v1/`, con ejemplos que comprueba `spec/schemas/check.py`.
  - [x] A.1b Reglas y rule sets (lógica, modo estricto, política, agregado, payloads).
    - Hecho el 2026-10-31 y revisado por `api-reviewer`, sin hallazgos graves. Decisiones tomadas:
      - Los valores del payload son siempre nodos. El orden de sus claves es normativo, y ningún binding puede pasarlo por un mapa sin orden.
      - Las etiquetas son un conjunto.
      - Las acciones quedan fuera; un escritor las rechaza con `unsupported` salvo con la opción `omit_actions` (D.4).
      - Los nombres de política, agregado y lógica se escriben en minúsculas y deben coincidir exactamente.
      - Una lógica desconocida da `registry_error`. Una lógica que existe pero falta en esta compilación da `unsupported`.
      - La forma de objeto de `logic`, con parámetros, y la clave `alpha` quedan reservadas para añadirlas sin cambiar de versión.
      - Los metadatos deben ser valores de datos.
      - Las diferencias con la v1 están en `json-format.md`, sección *Differences from the Python v1*.
  - [x] A.1c Decisiones y motivos (`Reason`, árbol de resultados).
    - Hecho el 2026-10-31 y revisado por `api-reviewer`, con un hallazgo grave corregido: no estaba definido cómo se lee una decisión. Decisiones tomadas:
      - La decisión guarda el árbol de resultados de cada regla, con `truth` para las condiciones y `value` para los valores, y los motivos estructurados.
      - Las salidas no se guardan, porque se deducen de los árboles del payload. `fired` y `aggregated` sí se guardan, y el lector los recalcula.
      - Una decisión se lee siempre con su rule set, y se comprueba nodo a nodo (`decision_mismatch`): solo se acepta lo que el núcleo pudo producir.
      - `ruleset_digest` y `metadata` quedan reservados.
      - El orden y los duplicados de `paths` y `types` en los motivos quedan fijados en `ast.md`.
      - Los grupos de tipo se escriben en *snake case*.
  - [x] A.1d Explicaciones (requisitos, `Counterfactual`) y resúmenes; `spec/explain.md`, que aún no existe.
    - Hecho el 2026-10-31 y revisado por `api-reviewer`, que confirmó que los algoritmos coinciden con la v1 línea a línea. Decisiones tomadas:
      - `spec/explain.md` especifica los estados, los requisitos `all`/`any`, la simplificación, la comprobación de opciones (con tope de 64), los contrafactuales y las notas.
      - El texto de `explain()` no es normativo: lo fijan los *snapshots* del núcleo, y la sintaxis de las condiciones en el texto se decide en B.3 y D.2.
      - Explicaciones y resúmenes son documentos JSON solo de salida.
      - Cada causa guarda su condición y un puntero `at` al nodo de la decisión del que sale.
      - La aritmética del hueco es propia y exacta: enteros comprobados en 64 bits, decimales exactos e instantes UTC.
      - Quedan reservados los campos de las explicaciones graduadas.
- [x] A.2 `spec/data-model.md`: `Value` y cómo convierte cada binding sus tipos nativos; precisión y rango de `Decimal`; reescribir `ast.md` en términos neutros. Se divide en:
  - [x] A.2a `spec/data-model.md`.
    - Hecho el 2026-10-31. Decisiones tomadas:
      - Decimales de 38 dígitos con un tipo propio en el núcleo: coeficiente i128 y escala de 0 a 38, como Arrow `Decimal128`, con infinitos. Lo eligió el autor frente a `rust_decimal`, que llega a 28 dígitos, y a la precisión arbitraria.
      - Los valores sin equivalente pasan a ser **opacos**, del grupo `other`: se comportan como en la v1 y la salida devuelve el objeto nativo.
      - Los datos fuera de rango dan un nulo con motivo: `int_overflow` y el nuevo `decimal_overflow`.
      - Las fechas con hora se truncan al microsegundo.
      - Los mapas sin orden definido se ordenan por clave.
      - Cada binding lee **solo las rutas que usa el rule set**, con las reglas de seguridad de `ast.md`.
      - Revisado por `api-reviewer`, con dos hallazgos graves corregidos:
        - Las listas y los mapas al final de una ruta siguen siendo el objeto nativo, que se devuelve tal cual en las salidas, y solo se convierten cuando hace falta.
        - La igualdad bajo `ANY` queda definida: listas elemento a elemento, mapas sin importar el orden y opacos por identidad.
      - El binding de Python lee exactamente como la v1; lo que no se puede conservar está en *Differences from the Python v1*.
      - Las zonas horarias se resuelven con la base de datos del núcleo, con un desfase fijo como alternativa.
      - Los opacos se escriben como `{"$opaque": "<tipo>"}`, y solo en decisiones y explicaciones.
  - [x] A.2b Reescribir `ast.md` en términos neutros: quedan clases Python, la columna de tareas de la v1 y notas de polars.
    - Hecho el 2026-10-31:
      - Sin nombres de clase ni tareas de la v1.
      - La lectura de datos se expresa sobre valores, y los detalles de Python pasan a `data-model.md`.
      - Se añade el grupo `other` a los tipos.
      - Las notas de polars pasan a ser notas informativas para el modo Arrow.
      - Las diferencias con la v1 quedan en una sección propia.
      - Revisado por `api-reviewer`, sin hallazgos graves y sin reglas perdidas. Se corrigieron tres incoherencias:
        - `type_mismatch` cubre también el grupo `other`.
        - Las constantes nulas se admiten bajo `is_null`, y las fechas con hora más finas que el microsegundo se rechazan.
        - Los namedtuples y las filas de pandas son **registros**: mapas con posiciones, indexables por entero como en la v1. La sección "Reading data" de `ast.md` (mapas, registros, atributos **públicos**, nunca privados ni de módulos o clases) pasa a ser una obligación de cada binding.
- [x] A.3 Exportador temporal en Python (`conformance/tools/`) que, con la implementación de la v1, genera **vectores de conformidad**: árboles y errores de construcción, evaluaciones con motivos, modo estricto, decisiones de las 6 políticas, agregados, explicaciones y resúmenes. Las propiedades de hypothesis se convierten en miles de casos fijados.
  - Se divide en:
  - [x] A.3a Formato de los vectores (`conformance/README.md` y `conformance/vectors.schema.json`), catálogo de errores (`spec/errors.md`) y exportador (`conformance/tools/`), con los codificadores de la v1 al formato JSON y una primera tanda de 122 vectores elegidos a mano: tablas de Kleene, lectura, ausentes, tipos, nodos, motivos, políticas, agregados, payloads, modo estricto, lotes, explicaciones y resúmenes.
    - Hecho el 2026-10-31. El exportador es determinista: ejecutarlo dos veces deja los vectores igual. `conformance/tools/check.py` valida cada documento contra los esquemas de la especificación y comprueba que las secciones citadas existen. Las herramientas de `conformance/tools/` no pasan por pyright estricto, porque son temporales y se borran en A.4.
    - Revisado por `api-reviewer`, con un hallazgo grave corregido: la distancia entre fechas con hora se codificaba en días cuando era de días enteros.
      - El exportador recalcula cada distancia con la aritmética de la especificación y salta los casos en que la v1 discrepa, como el cambio de hora.
      - Las entradas se codifican de forma estricta.
      - Los vectores de explicación incluyen su decisión.
      - `spec/errors.md` define cómo se comparan los errores, la ruta completa en `missing_field` y la forma idiomática en cada lenguaje: `*Exception` en Java, un solo `*Error` con código en Go, un `enum` en C y `Error` en Rust. Python conserva las bases de la v1.
      - Quedaron definidos en la especificación la API para evaluar un nodo (`evaluate_tree`), la escala de las distancias decimales, el resultado de `min`/`max` y qué no lanza el modo estricto.
  - [x] A.3b Vectores de lectura de documentos (léxicos, errores con su puntero y orden, límites), escritos desde la especificación, porque la v1 no tiene lector.
    - Hecho el 2026-10-31: 148 vectores en `conformance/vectors/format/`, generados por `conformance/tools/read_cases.py`.
      - Cubren documentos, números y sus bordes, decimales, fechas y zonas, valores con tipo, condiciones, reglas, rule sets, decisiones leídas con su rule set y límites.
      - `check.py` comprueba que cada puntero de error existe en el documento dado y que cada línea y columna cae dentro del texto.
      - La especificación precisa a qué carácter apunta un error de sintaxis, que un documento sin clave de contenido o con varias da error en la raíz, a qué nodo apunta un `expression_error` y que `registry_error` lleva puntero.
      - Revisado por `api-reviewer`, con dos hallazgos graves corregidos:
        - Un decimal con ceros finales cabe con escala 37, no 38; el ejemplo de `data-model.md` estaba mal.
        - La comprobación 4 de una decisión usa `selected` y `fired` tal como están escritos.
      - Tras la revisión hay 187 vectores de lectura con ids descriptivos. La especificación fija además:
        - los punteros de claves ausentes y desconocidas;
        - que el signo del cero se compara;
        - el final de texto y CRLF;
        - la profundidad de nodos en preorden, durante la pasada de forma;
        - las claves desconocidas, después de las conocidas;
        - el anidamiento JSON, que cuenta la cima como 1;
        - los floats que se redondean a infinito o a cero.
  - [x] A.3c Vectores generados con las propiedades de hypothesis (miles, con semilla fija).
    - Hecho el 2026-10-31: 1.821 vectores en `conformance/vectors/generated/`, producidos por `conformance/tools/generated.py`. Son 892 condiciones, 186 en modo estricto, 168 valores, 275 rule sets y 300 explicaciones.
      - Las estrategias se centran en pocas claves (`a`, `b`, `c` y `n.*`) y están ponderadas, para que los datos coincidan con los campos.
      - Hypothesis se ejecuta en modo determinista, sin base de datos ni reducción de casos, y los duplicados se eliminan.
      - Los ficheros generados llevan un vector por línea y ocupan 3 MB.
      - Cubren 24 tipos de nodo, las 6 políticas y los motivos `missing_field`, `null`, `type_mismatch`, `unorderable` y `division_by_zero`. `int_overflow`, `nan_result` y `decimal_overflow` los cubren vectores escritos a mano.
      - Regenerar con otra versión de hypothesis cambia los vectores generados, así que solo se regeneran durante la fase A, con la versión fijada en `uv.lock`.
  - [x] A.3d Script de cobertura: cada sección normativa citada por al menos un vector, y CI.
    - Hecho el 2026-10-31.
      - `check.py` exige que cada sección normativa esté citada por al menos un vector, ella o una subsección.
      - Las secciones que los vectores no pueden comprobar llevan la marca *Not checked by vectors* con el motivo: datos nativos, jerarquía de excepciones, texto de `explain()`, escritura con acciones, YAML, lógica difusa y lotes Arrow. Las diferencias con la v1 también quedan exentas.
      - Las citas que faltaban se asignan en `export.py` por patrones de id.
      - Hay dos vectores nuevos de datos fuera de rango: `int_overflow` y `decimal_overflow`; este último está escrito desde la especificación, porque la v1 no lo detecta.
      - El CI tiene un trabajo `conformance` que regenera los vectores, exige que no cambien y ejecuta las comprobaciones.
  - Vectores de lectura que hacen falta aunque la v1 no los pueda generar:
    - números: `-0`, `9223372036854775808`, `-9223372036854775809`, `18446744073709551616`, `{"$int": 1.0}`, `{"$int": 1e2}` y flotantes difíciles de redondear;
    - claves duplicadas en nodos, en mapas de datos y en `$map`;
    - claves de payload `"b"`, `"a"`, `"10"` y `"2"`, en ese orden, para comprobar que se conserva el orden;
    - bordes de los decimales: 38 y 39 dígitos, `1E+37` y `1E+38`, escala 39 y ceros finales;
    - enteros fuera de rango en los datos de entrada, que dan nulo con `int_overflow`;
    - truncado al microsegundo antes de 1970;
    - orden de mapas con claves fuera del plano básico de Unicode (punto de código frente a UTF-16);
    - surrogates sueltos;
    - el orden de errores;
    - los límites, exactamente en el borde.
  - Casos de la v1 que **no** se toman como vectores:
    - `Const(None)` bajo `and`, `or` y `not`;
    - patrones regex que Python acepta y `regex` no, y `$` antes de un salto de línea final;
    - la deduplicación de `in` con dos instantes distintos en la misma zona durante un cambio de hora, que la v1 fusiona porque Python compara horas locales cuando comparten `tzinfo`;
    - los huecos de contrafactuales que la v1 calculaba distinto (`explain.md`, *Differences from the Python v1*): enteros de más de 64 bits, decimales redondeados a 28 dígitos y fechas con hora restadas como hora local durante un cambio de hora.
- [x] A.4 Retirada: tag `python-reference` y eliminación de `src/rulepy` y de sus tests. Se conservan `spec/`, `docs/pitfalls.md` (pasa a la documentación del binding de Python) y los vectores.
  - Hecho el 2026-10-31.
    - El autor crea el tag `python-reference` sobre `09001aa`, el último commit con la v1, porque crear tags está denegado para Claude.
    - Se borran `src/rulepy`, `tests/`, la web de documentación (`mkdocs.yml`, `docs/index.md`, `docs/reference/`), `pyproject.toml`, `uv.lock` y el exportador de `conformance/tools/`. Se conserva `check.py`.
    - La configuración de ruff pasa a `ruff.toml`.
    - El CI se queda con el lint de las herramientas en Python y la comprobación de esquemas y vectores; el de Rust llega en B.1.
    - En el CHANGELOG, lo de la v1 pasa a una sección `python-reference`.
    - `docs/pitfalls.md` se queda en `docs/` hasta el binding de Python (fase C).

**Aceptación**: el conjunto de vectores cubre cada sección de la especificación; un script comprueba que cada regla normativa de `spec/` cita al menos un vector.

---

### Fase B. Núcleo Rust · tamaño XL · v0.1 (crate)

**Objetivo**: toda la funcionalidad de la fase P, en Rust, superando los vectores de conformidad.

- [x] B.1 Andamiaje: workspace, edición y MSRV, `clippy` pedante, `rustfmt`, `cargo-deny`, `cargo-nextest`, CI (Linux, macOS, Windows), `miri` en las partes con `unsafe` (idealmente ninguna en `core`).
  - Hecho el 2026-10-31.
    - Workspace con `crates/core` (crate `cleanrules`, aún sin dependencias), edición 2024 y MSRV 1.95.
    - Lints del workspace: `unsafe_code` prohibido, clippy pedante, y `unwrap`, `expect`, `panic`, `print*` y `dbg` vetados fuera de los tests (`clippy.toml`); se comprobó que rompen la compilación.
    - Configuración en `rustfmt.toml` (estilo 2024, 100 columnas) y `deny.toml` (licencias permisivas, solo crates.io).
    - CI de Rust: fmt, clippy y nextest en Linux, macOS y Windows; `cargo check` con 1.95; documentación con `-D warnings`; cargo-deny. Dependabot vigila también Cargo.
    - Desviaciones:
      - No hay trabajo de `miri`, porque el núcleo no tiene `unsafe`; llegará con `crates/ffi` (fase E).
      - nextest, cargo-deny, cargo-llvm-cov y la toolchain 1.95 no están instalados en local: el CI los usa, y en local basta `cargo test`.
- [x] B.2 Modelo de valores (`Value`, `Decimal` exacto propio de 38 dígitos según `spec/data-model.md`, fechas con `jiff` o `chrono`), normalización y grupos de tipo. Base de datos de zonas horarias **empaquetada** y fijada por versión (no la del sistema), con su versión expuesta en la API. Lector JSON propio o `serde_json` configurado: enteros exactos, `float_roundtrip`, detección de claves duplicadas y límite de anidamiento sin depender de la recursión por defecto.
  - Se divide en:
  - [x] B.2a Valores (`Value`, `Map`, `Opaque`, `NullCause`), decimales propios de 38 dígitos, comparación exacta entre enteros, floats y decimales, fechas y fechas con hora con zona sobre `jiff`, grupos de tipo, igualdad bajo `ANY`, y el tipo de error del núcleo con los códigos de `spec/errors.md`.
    - Hecho el 2026-10-31. El autor aprobó las dependencias `thiserror`, `jiff` (sin funciones por defecto, con `tzdb-bundle-always`), `serde`/`serde_json`, `regex` y `tracing`; de momento solo se añaden `thiserror` y `jiff`.
      - La comparación exacta entre float y decimal usa una aritmética entera mínima propia (`value/bignum.rs`).
      - La conversión de decimal a float pasa por el texto canónico, porque el lector de floats de Rust redondea correctamente.
      - Queda para B.8 la resta exacta de decimales para los contrafactuales.
      - La revisión de la API pública del crate se hará al final de la fase B, antes de la v0.1, porque hasta entonces cambiará mucho.
  - [x] B.2b Lector JSON propio (el autor lo prefirió a `serde_json`): posiciones en puntos de código, claves duplicadas, enteros exactos, floats con redondeo correcto, límites de tamaño y anidamiento.
    - Hecho el 2026-11-01: `cleanrules::json`.
      - Lector iterativo, con pila explícita, que conserva el orden de las claves y el texto de cada número.
      - Rechaza claves duplicadas, surrogates sueltos, BOM y UTF-8 inválido.
      - Da posiciones en puntos de código y aplica los límites de tamaño y anidamiento.
      - `Json` se destruye de forma iterativa, así que ni la lectura ni la destrucción desbordan la pila; se probó con 200.000 niveles.
      - El escritor es compacto.
      - `Error` va en una `Box`, para que `Result` ocupe un puntero.
      - Los tests de integración leen los 2.135 vectores con el propio lector y pasan los 12 vectores léxicos de `format/`.
  - [x] B.2c Lectura y escritura de valores en JSON (`$int`, `$float`, `$decimal`, `$date`, `$datetime` con zonas, `$map`, `$opaque`), contra los vectores de `format/values`.
    - Hecho el 2026-11-01: `cleanrules::format::value`.
      - Tres contextos de lectura: constante, datos (los desbordamientos dan nulos con motivo) y valores registrados (admiten opacos).
      - Las fechas con hora siguen la gramática de la especificación. Las zonas IANA se validan contra la base empaquetada de `jiff`, distinguiendo mayúsculas y conservando los alias, y se comprueba que el desfase coincide con el de la zona en ese instante.
      - La escritura es canónica.
      - Pasan los 79 vectores de `format/values`. Se comprobó que el test detecta expectativas alteradas, incluido el signo de `-0.0`.
    - Pendiente: exponer en la API la versión de la base de datos de zonas (`spec/json-format.md`, *Time zones*); se hará cuando exista una API pública de documentos (B.3).
- [x] B.3 AST con serde: los 14 tipos de nodo y sus validaciones al construir, identidad estructural, `field_paths`, `Display` legible.
  - Hecho el 2026-11-01.
    - `cleanrules::ast`: `Node` solo se construye con constructores que aplican las reglas de `spec/ast.md`, así que no puede existir un árbol inválido; `Expr` expone su forma para leerla.
    - Igualdad y hash estructurales: el entero 1 y el float 1.0 son nodos distintos, mientras que 0.0 y -0.0 son el mismo. Incluye `field_paths` en preorden.
    - Los valores de `in` se ordenan y deduplican. `matches` compila con `regex`, que rechaza lookaround y retroreferencias.
    - `cleanrules::format`: `read_document` y `write_document` para documentos de condición, con el orden de errores de la especificación: forma en preorden y luego construcción en postorden, con punteros.
    - Pasan los 150 vectores de `format/documents`, `values`, `conditions` y `limits`. Se comprobó que el test detecta expectativas alteradas. Las ayudas de los tests están en `tests/common`.
    - Desviaciones:
      - No se usa serde, porque el lector JSON es propio (B.2b).
      - El `Display` de las condiciones se deja para la sintaxis del DSL (D.2), que también fijará el texto de `explain()`.
      - El lector de nodos es recursivo hasta `max_depth` (256 por defecto). Si alguien sube mucho ese límite, necesitará más pila; se revisará antes de la v0.1.
      - Sigue pendiente exponer la versión de la base de zonas horarias.
- [x] B.4 API de construcción idiomática en Rust (constructores y sobrecarga de operadores donde tenga sentido; macros solo si aportan).
  - Hecho el 2026-11-01: `cleanrules::build`. Por ejemplo, `(field("customer").get("years").gt(5) & field("order").get("total").gt(1000)).build()?`.
    - Comparaciones y tests como métodos (`gt`, `is_in`, `between`, `matches`…), porque Rust no deja sobrecargar `<` y `>` para construir nodos. Conectivas y aritmética como operadores (`&`, `|`, `!`, `+`, `-`, `*`, `/` y el menos unario).
    - `Build` arrastra el primer error, por la izquierda, y se comprueba una vez con `.build()?`.
    - Conversiones desde `bool`, enteros, `f64` (NaN es un error), texto, `Decimal` y fechas de `jiff`.
    - Sin macros, porque no aportaban.
- [x] B.5 Lógicas: `Logic` (trait), Kleene, registro por nombre y capacidad `Graded`.
  - Hecho el 2026-11-01: `cleanrules::logic`.
    - El trait `Logic` tiene un tipo de verdad asociado, las conectivas, la lectura de un dato como condición y la prueba de activación.
    - `Kleene` está registrada como `"boolean"`. `LogicName::from_name` devuelve `registry_error` con el nombre y los disponibles.
    - Las tablas de la especificación y las leyes (De Morgan, doble negación, asociatividad, distributividad) se prueban de forma exhaustiva sobre las 27 combinaciones.
    - Desviación: la capacidad `Graded` se define en la fase J, con su primera lógica difusa, en lugar de dejar ahora un trait vacío.
- [x] B.6 Evaluador: lectura de `Value` por rutas, nulos, tipos, aritmética (float64, desbordamiento de 64 bits, `Decimal`), todos los nodos, árbol de resultados con `Reason`, modo estricto y orden de errores.
  - Hecho el 2026-11-01: `cleanrules::eval` (`evaluate_tree` y `evaluate_value_tree`, genéricos sobre `Logic`), y el árbol de resultados en JSON (`format::write_result`).
    - Lee rutas con sus tres desenlaces: valor, ausente con el prefijo y nulo en mitad de la ruta.
    - Los nulos con motivo vienen de los datos. Hay tipos, motivos con rutas y grupos en el orden de la especificación, y las mitades sintéticas de `between`.
    - Aritmética: desbordamiento de 64 bits a nulo, división en float y decimales convertidos a float.
    - El modo estricto lanza el primer error en postorden con la ruta completa.
    - `tests/conformance.rs` es el runner de conformidad del núcleo: pasan **los 1.326 vectores** de tipo `condition` y `value`, escritos a mano y generados, a la primera. Se comprobó con cuatro expectativas alteradas que el runner las detecta.
    - Pendiente:
      - Los registros (mapas con posiciones, como namedtuples y filas de pandas) aún no existen como `Value`; llegan con el binding de Python (C.3).
      - El JSON de los valores de verdad solo existe para Kleene; las lógicas difusas lo añadirán en la fase J.
- [x] B.7 Reglas, rule sets, políticas DMN, payloads, agregados y `Decision`.
  - Se divide en:
  - [x] B.7a Reglas, rule sets, las 6 políticas, payloads, agregados, lotes y `Decision`, con sus documentos JSON (reglas y rule sets en lectura y escritura, decisiones en escritura).
    - Hecho el 2026-11-01: `cleanrules::rules` y `format::{read_ruleset, write_rule, write_ruleset, write_decision}`.
      - Mismo orden que la v1:
        1. todas las condiciones;
        2. con `any`, los payloads de las reglas que dispararon;
        3. la selección, donde `unique` lanza su error;
        4. los payloads de las reglas seleccionadas;
        5. la igualdad de `any`;
        6. el agregado.
      - `sum` sigue la aritmética de Python: el entero es exacto hasta que aparece un float, y si algún valor es decimal todos pasan a float.
      - `Aggregated` es un enum, en lugar de `Option<Option<Map>>`.
      - El runner pasa **los 302 vectores de evaluación** a la primera, 1.628 en total. El test de lectura pasa los de reglas y rule sets, 178 en total. Se comprobó con expectativas alteradas que los runners las detectan.
  - [x] B.7b Lectura de decisiones con su rule set y las 8 comprobaciones de `decision_mismatch` (vectores `format/decisions`).
    - Hecho el 2026-11-01: `format::read_decision_document(text, ruleset, limits)`.
      - Reconstruye una `Decision` real, cuyos árboles apuntan a los nodos del rule set, y la comprueba en el orden de la especificación.
      - Para la comprobación 5 reutiliza el evaluador (`eval::recompute`); para la 7 y la 8, la selección y el agregado de `RuleSet`.
      - Pasan los 187 vectores de lectura.
      - Además, el runner relee cada decisión evaluada: las 302 superan las 8 comprobaciones y se reescriben idénticas.
      - Corrección del 2026-11-01: esa prueba de ida y vuelta no llegó a insertarse en el runner en B.7b, porque el script que la añadía falló antes. Se añadió y comprobó en B.8, y pasa.
- [x] B.8 `explain` (requisitos `all`/`any`, De Morgan, fusión y absorción, validación de opciones con apariciones vinculadas, contrafactuales) y `summary`.
  - Hecho el 2026-11-01: `cleanrules::explain` (`explain`, `summary`) y `format::{write_explanation, write_summary}`.
    - Estados, requisitos `all`/`any` con De Morgan, simplificación (aplanar, fusionar y absorber), y comprobación de las opciones que hacen disparar la regla, con un tope de 64 y la detección de conflictos.
    - Causas con los valores leídos y contrafactuales con la aritmética exacta de la especificación (`Decimal::checked_sub`; fechas con hora en UTC), y notas.
    - El puntero `at` se obtiene por identidad del resultado en la decisión.
    - Pasan **los 317 vectores de explicación y los 3 de resumen** a la primera; en total, 1.948 vectores en el runner más 187 de lectura. Se comprobó con cinco expectativas alteradas que el runner las detecta.
    - Cada decisión, evaluada o explicada, se relee con su rule set y supera las 8 comprobaciones; la releída se explica igual.
    - Desviación: el texto de `explain()` y de `summary()` se deja para D.2, con la sintaxis del DSL.
- [x] B.9 Runner de conformidad en Rust y tests de propiedades con `proptest` (leyes lógicas, De Morgan, suficiencia de las explicaciones).
  - Hecho el 2026-11-01: el runner de conformidad ya existía desde B.4 (`crates/core/tests/conformance.rs`); se añadió `crates/core/tests/properties.rs` con 6 propiedades, 512 casos cada una.
    - Generadores: escalares (int, float, decimal, bool, texto, fecha) y nulos en los campos `a`, `b` y `c` (a veces ausente), y árboles de condiciones de hasta 3 niveles construidos con los constructores de `Node`.
    - Propiedades: la evaluación no estricta nunca falla y da razón justo cuando la verdad es desconocida o el valor es nulo; el modo estricto coincide o lanza un error de la familia de evaluación; De Morgan y la doble negación; ida y vuelta de las condiciones por JSON; toda decisión se relee (cuatro políticas); y la suficiencia de las explicaciones (`selected`: todas las causas se cumplen; `not_fired` alcanzable: ninguna; `unknown`: todas desconocidas).
    - Pasan a la primera, sin contraejemplos que convertir en vectores. Se comprobó con dos propiedades alteradas que proptest las detecta y reduce el contraejemplo.
- B.10 Benchmarks con `criterion` (evaluación por lote, explicación), comparados con zen-engine. Se divide en dos subtareas:
  - [x] B.10a Suites `eval` y `explain` con `criterion`, y la primera medición.
    - Hecho el 2026-11-01: `crates/core/benches/` con una carga común (12 reglas de admisión de préstamos que usan todas las familias de nodos y 1.000 solicitantes deterministas, con campos ausentes o nulos).
    - Casos: una entrada y lotes de 1.000 con `first`, `priority` y `collect`; un árbol de condición; lectura y escritura del rule set en JSON; explicación de 1.000 decisiones, con y sin JSON; evaluar y explicar juntos; y el resumen.
    - Medición en `benchmarks/results/2026-11-01-core.json` (Apple M5, rustc 1.97.1), guardada como línea base de criterion `2026-11-01`: unos 7,2 µs por entrada con `first` (12 reglas), 11,5 µs por explicación y 58 µs con su JSON.
    - Desviación: `criterion` sin las características por defecto (sin gráficos ni rayon), para no arrastrar dependencias.
    - Pendiente para optimizar (con benchmark, en una tarea propia): escribir la explicación en JSON cuesta unas 4 veces más que construirla.
  - [x] B.10b Comparación con zen-engine con el mismo rule set como tabla de decisión JDM, en un crate aparte en `benchmarks/` fuera del workspace.
    - Hecho el 2026-11-01: `benchmarks/zen` (zen-engine 2.1.6, con su propio `[workspace]`; no se compila en CI). Reutiliza la carga de `crates/core/benches/common` y expresa las 12 reglas como una tabla de decisión compilada, sin traza.
    - Antes de medir comprueba que los dos motores seleccionan las mismas reglas para los 1.000 solicitantes con `first` y con `collect`, y que zen no modifica la entrada. Coinciden en todos; se comprobó con una fila alterada que la comprobación lo detecta.
    - Resultado en `benchmarks/results/2026-11-01-zen.json`: 1.000 evaluaciones en 7,22 ms frente a 46,4 ms con `first` (6,4×) y 8,51 ms frente a 56,7 ms con `collect` (6,7×), sin solapamiento de intervalos. cleanrules hace más trabajo (evalúa todas las reglas y guarda los árboles para explicar). Es una sola carga en una sola máquina.
    - Desviación: en macOS 27, el `strip` de Xcode deja inservible la proc macro `rquickjs-macro` (de zen) en perfil release (E0463), así que el crate desactiva `strip` en `profile.release.build-override`.

**Aceptación**: el 100 % de los vectores en verde; `clippy -D warnings`; sin `unsafe` en `core`.

Cumplida el 2026-11-01: pasan los 2.135 vectores (1.948 en el runner y 187 de lectura), `clippy -D warnings` está limpio y `core` prohíbe `unsafe` (`unsafe_code = "forbid"`). Antes de publicar la v0.1 del crate quedan la revisión de la API y el lector de nodos iterativo.

---

### Fase C. Binding Python · tamaño L · primera versión pública

- [x] C.1 PyO3 + maturin, wheels `abi3` y build *free-threaded*, CI de wheels.
  - Hecho el 2026-11-01: `bindings/python` (crate `cleanrules-python`, miembro del workspace; paquete `cleanrules` con el módulo nativo `cleanrules._cleanrules`), con PyO3 0.30.1 y maturin 1.16. De momento solo expone `__version__` y `FORMAT_VERSION`; la API llega en C.2.
    - Tres wheels por plataforma, como permite PyO3 0.30: `cp311-abi3` (CPython 3.11 a 3.14 con GIL), `cp314-cp314t` (3.14 *free-threaded*; PyO3 ya no admite 3.13t) y `cp315-abi3.abi3t` (PEP 803: 3.15 y posteriores, con GIL o sin él). Se construyeron en macOS arm64 y sus tests pasan en 3.11, 3.14, 3.14t, 3.15 y 3.15t (rc1). El sdist también se instala.
    - El módulo admite *free threading* (lo predeterminado en PyO3): un test comprueba que importarlo no reactiva el GIL, y falla si el módulo lo pide (comprobado).
    - CI: un trabajo `python` en `ci.yml` (pytest en Linux, macOS y Windows con 3.11, 3.14t, 3.15 y 3.15t, y pyright) y `wheels.yml`, que construye las wheels (manylinux y musllinux x86_64/aarch64, macOS arm64 y x86_64, Windows x64 y arm64) y el sdist y prueba cada wheel instalada. Aún no se han ejecutado en GitHub: lo harán con el primer push.
    - Dependencias: `pyo3` (aprobada) y, como herramientas de desarrollo del paquete, `pytest` y `pyright` (`uv.lock` en `bindings/python`).
    - Desviación: el `strip` de Xcode 27 corrompe a veces bibliotecas Mach-O (la wheel `abi3t` no cargaba: «mis-aligned LINKEDIT string pool»), así que el perfil release del workspace no hace `strip`, también en `build-override`. Cuesta un 8 % de tamaño (37 KB).
- C.2 API Python con la ergonomía de la v1: `F`, operadores, protección contra trampas (`docs/pitfalls.md`), `Rule`, `RuleSet`, `Decision`, `explain`, `summary`. Se divide en dos subtareas:
  - [x] C.2a Expresiones y errores: `F`, `Field`, `Const`, operadores y métodos, guardas de las trampas, `repr` como código Python, y la jerarquía de excepciones de `spec/errors.md`.
    - Hecho el 2026-11-01: cada nodo lo construye el núcleo, que aplica las reglas de construcción. En Python quedan los operadores, las guardas y el `repr`, portados de la v1 (`python/cleanrules/_expr.py`); las excepciones están en `python/cleanrules/errors.py`, y el módulo nativo elige la clase por código y pasa el contexto.
    - Las constantes se convierten en Rust según la tabla de Python de `spec/data-model.md`: enteros de 64 bits, decimales exactos de hasta 38 dígitos (nunca redondeados), NaN rechazado, fechas, datetimes con zona IANA o desplazamiento fijo, y subclases (`StrEnum`, `IntEnum`).
    - Núcleo: `AwareDateTime::from_native`, que conserva la zona IANA solo si la base de datos del núcleo la conoce y coincide el desplazamiento, y la reexportación de `jiff`, cuyos tipos ya forman parte de la API pública.
    - Los nodos se copian como sí mismos y se serializan con pickle a través del formato JSON (`node_from_json`).
    - 166 tests (los de la v1, adaptados, y propiedades con hypothesis: el `repr` se reevalúa al mismo árbol, pickle conserva el árbol y la clase). Pasan en 3.11, 3.14 y 3.14t; pyright estricto y ruff limpios.
    - Desviaciones: los nodos son una sola familia (`Field`, `Const` y un nodo genérico hermano), no una clase por operador; las comparaciones entre nodos conservan su orientación. Siguiendo la especificación, la constante nula es de tipo `value` (la v1 decía `any`). Dependencia de desarrollo nueva: `hypothesis`.
  - [x] C.2b `Rule`, `RuleSet`, `Decision`, `explain` y `summary`, con una conversión provisional de los datos de entrada (la definitiva es C.3).
    - Hecho el 2026-11-01: `python/cleanrules/_rules.py` (`HitPolicy`, `Truth`, `Rule`, `RuleSet` como secuencia por posición y por nombre, `RuleResult`, `Decision`, `Explanation`, `RuleExplanation`, `Summary`) sobre las clases nativas de `src/rules.rs`.
    - Una `Decision` del núcleo toma prestado su rule set, así que la de Python guarda el rule set (compartido con `Arc`) y la entrada convertida, con lo que se lee de inmediato (resultados, seleccionadas, salidas). `explain()` y `to_dict()` evalúan otra vez la entrada: la evaluación es determinista (invariante 7).
    - `explain()` devuelve la explicación estructurada (la forma JSON de la especificación, con accesos por regla y `causes`); su texto llega en D.2, con el DSL. `to_dict()` devuelve documentos completos (`version` más `decision`, `explanation` o `summary`), así que los punteros `at` de una explicación se resuelven en el documento de la decisión (hay un test).
    - Conversión provisional de la entrada: dicts, listas, tuplas y escalares; enteros fuera de 64 bits, decimales que no caben y NaN pasan a nulos con su motivo; lo demás es opaco. La lectura dispersa de la especificación llega en C.3.
    - 202 tests en 3.11, 3.14 y 3.14t (uno evalúa el mismo rule set desde 8 hilos); pyright estricto, ruff y clippy limpios.
    - Desviaciones respecto a la v1: `evaluate_many` devuelve una lista (los errores llevan `row`), no un iterador; `Rule` aún no tiene `do` (acciones, C.4) y aún no se serializa con pickle (C.4).
    - Revisión de la API (subagente `api-reviewer`), aplicada:
      - Bloqueante: convertir datos que se contienen a sí mismos o con 200.000 niveles tumbaba el intérprete. Ahora la conversión tiene un límite de 256 niveles y detecta ciclos; lo que pasa de ahí es opaco, como cualquier elemento que no se puede convertir. Las fichas de los valores opacos se numeran por orden de aparición, nunca por dirección de memoria (invariante 7).
      - `evaluate_many` rechaza cadenas, mapas y data frames (`spec/rules.md`).
      - `explain()` devuelve objetos con valores de Python (`Cause` con su nodo, `Truth`, `holds`, los valores leídos, `Reason` y `Counterfactual`, con huecos `Decimal` o `timedelta`; `CauseGroup`; `Note`; `Status`), no la codificación JSON, que sigue en `to_dict()`.
      - `Summary` expone `FieldUsage` (qué reglas y qué cargas leen cada campo), `rules` y la configuración.
      - Los metadatos se validan como datos (sin NaN ni valores opacos, con la ruta de la clave en el error) y `Rule.metadata` devuelve lo que guarda el núcleo.
      - Salidas y decisiones de solo lectura; los errores de constantes de la carga nombran la regla y la clave; se permiten claves vacías, como en el formato; `hit_policy` y `aggregate` aceptan cualquier capitalización, como la v1; `Aggregate` es un `StrEnum`; la evaluación suelta el GIL en las compilaciones con GIL; un código de error nuevo se traduce a `CleanRulesError`; la pista del núcleo para salidas múltiples ya no usa sintaxis de método.
      - Pendiente (tarea propia, abajo): los campos de contexto `node`, `operation`, `reason` y `decision` de `spec/errors.md` no existen aún en el `Context` del núcleo.
  - [x] C.2c Campos de contexto de error que faltan en el núcleo (`node` en `expression_error` y en los errores de evaluación, `operation` y `reason` en `unsupported`, `decision` en `action_error`), y su paso a Python (`node` como `Expr`). No se comparan en los vectores.
    - Hecho el 2026-11-01: el `Context` del núcleo tiene `node`, `operation` y `reason`, y `Error::with_node`.
    - En la construcción, `node` es el operando que rompe la regla (las comprobaciones comunes de operandos lo añaden). En la evaluación estricta es el nodo más interno que falla: el campo, si falta; la comparación, si los tipos no casan; el `between` del usuario, si falla una de sus mitades sintéticas.
    - Python recibe `node` como `Expr` (también al deserializar con pickle) y lo muestra en el mensaje (`…, in \`F.a > 1\``); `operation` y `reason` pasan tal cual.
    - Rendimiento medido con criterion contra el código anterior en las mismas condiciones: sin cambio en `evaluate/one/first` ni en `batch_1000/first`. Un primer intento que clonaba las mitades de `between` costaba un 5 %, y se descartó.
    - Desviación: `decision` en `action_error` llega con las acciones (D.4), porque hasta entonces nadie lanza ese error.
- C.3 Conversión de datos (dicts, dataclasses, Pydantic, namedtuples, filas de pandas, numpy) a `Value`, con las reglas de seguridad de `spec/data-model.md`. Se divide en dos subtareas:
  - [x] C.3a Lectura dispersa de datos nativos: solo las rutas que lee el rule set, cada objeto una vez, registros, objetos, reglas de seguridad, `read_error` encadenado y objetos opacos devueltos.
    - Hecho el 2026-11-01: `src/read.rs`. Al construir un rule set se forma el árbol de las rutas que leen sus condiciones y cargas (del `summary` del núcleo); cada entrada se recorre por ese árbol y el núcleo recibe un árbol de valores disperso. Donde termina una ruta, el valor se convierte entero (con límite de profundidad y detección de ciclos).
    - Formas: mapas (`Mapping`, sin llamar nunca a `__missing__`), registros (namedtuples y objetos con `keys()`, como las filas de pandas), secuencias (índices `0 <= i < len`, nunca en cadenas ni mapas) y objetos (atributos públicos de datos, propiedades incluidas; dataclasses, attrs y Pydantic dan sus campos al convertirse enteros). Nunca un nombre privado, un método, una clase, un módulo ni un atributo de un escalar o de una secuencia.
    - Un getter que lanza da un nulo con `read_error`, salvo `AttributeError`, que es ausente; en modo estricto, el `EvaluationError` lleva como `__cause__` la excepción original (también en lotes).
    - Los valores opacos guardan su objeto nativo y vuelven en las salidas y explicaciones como el mismo objeto.
    - Núcleo: los registros, mapas cuyas entradas tienen posición (`Map::record`, `get_index`); un segmento entero lee la posición en un registro y nunca en un mapa normal (`spec/ast.md`).
    - 231 tests en 3.11, 3.14 y 3.14t (14 nuevos de lectura, uno por regla de la especificación); clippy, pyright y ruff limpios.
    - Desviación: las listas dispersas solo llegan hasta el último índice leído (nadie lee su longitud). La pista de `missing_field` con las claves disponibles queda para más adelante.
  - [x] C.3b numpy y pandas (escalares de numpy, `datetime64`, `NaT`, `pandas.NA`, arrays como listas), y tests con numpy, pandas, Pydantic y attrs instalados (dependencias de desarrollo que hay que aprobar).
    - Hecho el 2026-11-01 (dependencias de desarrollo aprobadas por el autor: numpy, pandas, Pydantic y attrs; los usuarios no las necesitan).
    - `pandas.NA`, `pandas.NaT` y el `NaT` de numpy son nulos; `numpy.datetime64` es una fecha con hora sin zona, truncada al microsegundo (fuera de los años 1 a 9999, opaca); `numpy.timedelta64`, que numpy declara entero, es opaco y no rompe la lectura.
    - Ya funcionaban, y ahora tienen tests: escalares de numpy, arrays como listas, `pandas.Timestamp` con nanosegundos o con zona, filas de `iterrows()` (registros con `keys()`) e `itertuples()` (namedtuples), y modelos de Pydantic (por nombre de campo y no por alias, con campos extra y calculados) y clases de attrs.
    - 251 tests en 3.11, 3.14, 3.14t, 3.15 y 3.15t (rc1), sin ninguno omitido.
- [x] C.4 `run` y acciones en Python; pickle; tipos (`.pyi`).
  - Hecho el 2026-11-01: `Rule(..., do=acción)` y `RuleSet.run(data)`, que evalúa y llama a las acciones de las reglas seleccionadas en orden de regla con un `ActionContext` (`rule`, `data`, `result`, `output`, `decision`); `evaluate` nunca las ejecuta. La primera que lanza para el `run` con `ActionError` (`rule`, `executed`, `decision`, y la excepción original como `__cause__`).
  - `Rule` y `RuleSet` se serializan con pickle y se copian cuando su acción también (las condiciones viajan en el formato JSON); el `decision` de un `ActionError` no se serializa.
  - Tipos: `pyright --verifytypes` da un 100 % de completitud en la API pública (antes un 83 %, por dos asignaciones de métodos sin anotar) y se comprueba en CI.
  - 258 tests en 3.11, 3.14, 3.14t y 3.15t.
  - Desviación respecto a la v1: el `ActionContext` lleva el `RuleResult` de la regla (`result`) en vez del árbol de resultados de su condición (`tree`), que Python aún no expone; y añade `decision`.
- [x] C.5 Runner de conformidad en Python.
  - Hecho el 2026-11-01: `bindings/python/tests/test_conformance.py`, con pytest, un caso por vector y pasada. Cada vector corre dos veces, como pide `conformance/README.md`:
    - `json`: por las entradas JSON del binding (`_cleanrules._read_document`, `_evaluate`, `_explain`, …; privadas hasta D.1). Comprueba que documentos y errores llegan a Python intactos.
    - `native`: la entrada se convierte en valores nativos de Python (`Decimal`, `datetime` con su zona, `dict`…), las condiciones y reglas se construyen con `F`, `Const`, operadores y `Rule`, y el resultado vuelve por la API de Python. Prueba el código propio del binding.
  - API nueva que pide la especificación (`spec/ast.md`, *Evaluating one node*): `evaluate_tree(condition, data, *, logic, strict)` y `evaluate_value_tree(value, data, *, strict)`, que devuelven un `ResultTree` (nodo, verdad o valor, motivo, hijos, `to_dict()`).
  - Resultado: pasan los 2.135 vectores por JSON y los 1.948 que no son de lectura en nativo (4.084 casos), en 3.11, 3.14, 3.14t, 3.15 y 3.15t. Se comprobó con un fallo provocado en el lector nativo (las claves ausentes leídas como nulos) que la pasada nativa lo detecta: 851 casos fallan.
  - El runner conserva el texto de los números al releer los documentos (`1e-7` no pasa a `1e-07`), para que los vectores de límites de tamaño cuenten los mismos bytes.
  - CI: el trabajo de pruebas de `wheels.yml` instala las dependencias de los tests (hypothesis, numpy, pandas…), que faltaban desde C.2.
  - Desviaciones: los vectores de lectura solo tienen la pasada JSON (la nativa no lee documentos); el texto de `explain()` y `summary()` llega con D.2.
  - Revisión de la API de C.2c a C.5 (subagente `api-reviewer`), aplicada:
    - Bloqueante: una regla podía leer variables globales y locales del programa a través de frames, tracebacks y generadores (`F.rec.exc_info[2].tb_frame.f_globals…`). Esos objetos del intérprete, y las funciones, son ahora opacos y nunca se lee a través de ellos; `spec/data-model.md` (*Safety*, *Python*) lo dice.
    - Bloqueante: una decisión con objetos opacos en un ciclo de referencias no se liberaba. `PyDecision` implementa `__traverse__` y `__clear__` (los objetos van tras un `Mutex` que solo se toma para copiarlos, sin ejecutar Python, y el recolector usa `try_lock`).
    - `KeyError` solo significa ausente al leer un elemento, no un atributo; las filas con `_fields` que no son tuplas (SQLAlchemy 2) son registros; los arrays de numpy (también `matrix`) se leen por `tolist()`; los escalares de numpy se reconocen por `numpy.generic`; una excepción al averiguar la forma de un valor (un `__len__` que lanza) es un `read_error`; `keys()` se pide una vez por registro; las fichas opacas usan un `HashMap`; las listas dispersas conservan su longitud.
    - Núcleo y especificación: un `read_error` en mitad de una ruta ya no pasa por un nulo corriente (`spec/ast.md`, *Missing values*): da `read_error` y, en modo estricto, un `EvaluationError` encadenado. `Error::unsupported(operation, reason)`. El mensaje «a add is not a condition» pasa a «'add' is not a condition».
    - `ActionContext.tree` (el árbol de resultados de la condición, sin volver a leer los datos); el mensaje de `ActionError` ya no repite la regla; `evaluate_tree` elige la lógica con un `match`; `ResultTree` no expone su JSON en el constructor; `node` y `decision` de los errores tienen tipo; el docstring de `Rule.tags` dice que van ordenadas, como hace el núcleo.
    - Desviaciones anotadas: donde termina una ruta, las listas y mapas se convierten enteros en el momento (la especificación permite referencias nativas perezosas; queda como optimización); las claves no textuales de un mapa se descartan en vez de volverse opacas, porque una ruta no puede leerlas.

**Cumplida la fase C el 2026-11-01**: el binding de Python tiene la API de la v1, lee datos nativos como dice la especificación, ejecuta acciones, se serializa con pickle, tiene tipos completos y pasa todos los vectores en sus dos pasadas. La publicación en PyPI (la «primera versión pública») la hace el autor cuando decida; antes conviene la revisión de la API de la v0.1.

---

### Fase D. Reglas como datos · tamaño M

- [x] D.1 Carga y volcado JSON y YAML en el núcleo (todos los lenguajes la heredan).
  - Hecho el 2026-11-01. Especificación primero: la sección *YAML* de `spec/json-format.md` precisa qué puede tener un fichero (un documento; nada de anclas, alias, etiquetas ni claves no escalares), cómo se resuelven los escalares planos (el subconjunto JSON del esquema central de YAML 1.2: `null`/`~`/vacío, `true`/`false`, la sintaxis numérica de JSON y, todo lo demás, texto), los errores con línea y columna y la forma canónica al escribir.
  - Núcleo: la *feature* `yaml` (con `saphyr-parser` 0.3.3, aprobada por el autor; desactivada por defecto) añade el módulo `cleanrules::yaml` (`parse`, `to_string`, como `cleanrules::json`) y `format::{read_yaml_document, read_yaml_decision_document}`; `read_parsed_document` y `read_parsed_decision_document` leen documentos ya convertidos a JSON, así que YAML pasa por las mismas comprobaciones, punteros y límites que JSON.
  - 32 vectores nuevos (`conformance/vectors/format/yaml.json`, campo `yaml` en los vectores de lectura, aditivo): escalares, estilos, claves, errores con posición, límites. Pasan en el núcleo y en Python; se comprobó con un lector alterado (`yes` como booleano) que fallan. CI compila y prueba con `--all-features`.
  - Python: `loads`, `load`, `dumps` y `dump` (como el módulo `json`), en JSON o YAML (`load`/`dump` deducen el formato del sufijo). Devuelven `Expr`, `Rule` o `RuleSet` construidos con la API pública; las reglas con acción no se escriben salvo con `omit_actions=True` (`UnsupportedError` con el puntero de la acción). El runner de conformidad lee ahora los vectores de lectura con esta API pública.
  - 4.398 tests de Python en 3.11, 3.14, 3.14t y 3.15t; clippy, pyright, ruff y la comprobación de Rust 1.95 limpios.
  - Desviación: la lectura de decisiones sigue privada en Python (`_read_decision`), porque una `Decision` de Python se reconstruye evaluando su entrada; queda para cuando haga falta. El YAML canónico escribe todas las secuencias en bloque, también las rutas cortas.
  - Revisión de la API (subagente `api-reviewer`), aplicada:
    - Bloqueantes: (1) el YAML canónico no podía releer claves de más de 1.024 caracteres (el máximo de YAML para una clave implícita): ahora se escriben como clave explícita (`? clave`). (2) La comprobación de claves duplicadas era cuadrática (26 s para 160.000 claves): ahora usa un `HashSet`. (3) `saphyr-parser` guarda entera cada colección de estilo flujo antes de emitir eventos (un YAML de 15 MB llegaba a 1,9 GB): los textos YAML tienen su propio límite de tamaño, 1 MiB por defecto (`Limits::max_yaml_size`), que la especificación explica.
    - Las directivas (`%YAML`, `%TAG`) y los caracteres de control sin escapar quedan prohibidos, con su posición; la marca de orden de bytes se admite, como en YAML; las colecciones de flujo anidadas más de 255 niveles dan `limit_exceeded`; las líneas cuentan `\r`, `\r\n` y `\n`. Los mensajes llevan pistas y el de claves duplicadas sigue al de JSON.
    - Hypothesis encontró, al aplicar la revisión, que el escritor dejaba un DEL sin escapar, que el lector rechaza ya: el escritor escapa ahora todo carácter de control como `\uXXXX`.
    - Vectores nuevos (38 en total de YAML): directiva, carácter de control, marca de orden de bytes, clave larga explícita, anidamiento de flujo y una decisión leída en YAML, que ambos runners leen ya con su rule set.
    - Python: `load` y `dump` aceptan rutas y ficheros abiertos, como el módulo `json`; `loads` acepta `bytes`; `load` comprueba el tamaño antes de leer; un límite negativo es un `ValueError`.
    - Nombres de Rust alineados con JSON; CI compila la *feature* por su nombre (`--features cleanrules/yaml`) en vez de todas.
    - Ejemplo en `docs/examples/discounts.{yaml,json}`, que un test mantiene canónico.
- D.2 DSL de texto (`IF ... THEN ...`, sintaxis alineada con `fuzzytool.rule_from_text`) con parser propio en Rust y errores con línea y columna. Se divide en dos commits:
- [x] D.2a Sintaxis de texto: especificación, parser e impresión canónica en el núcleo, vectores y Python.
  - Hecho el 2026-11-01. Decisiones del autor: los literales con fracción son decimales exactos (`0.15`) y los floats llevan exponente (`1.5e0`); `=` y `==` son la igualdad; cabecera `RULE nombre [PRIORITY n] [TAGS …] [DESCRIPTION "…"]`; `insta` como dependencia de desarrollo (se usa en D.2b).
  - Especificación nueva, `spec/text.md`: léxico (palabras clave sin distinguir mayúsculas, nombres ASCII o entre comillas invertidas, cadenas con los escapes de JSON o entre comillas simples, números, `DATE`/`DATETIME`/`DECIMAL`/`FLOAT "…"`), tabla de precedencia, reglas y rule sets (cabeceras `HIT POLICY`, `AGGREGATE`, `LOGIC`, `STRICT`; sin cabecera `RULE`, la regla se llama `rule_<n>`), errores (`rule_syntax_error` con línea y columna en puntos de código; los de construcción, en la posición del nodo) y la impresión canónica, que relee el mismo árbol. `IS` queda reservado a `IS [NOT] NULL` y, en la fase J, a los términos difusos. Los metadatos no se escriben en texto (`unsupported`).
  - Núcleo: módulo público `cleanrules::text` (`parse`, `parse_rule`, `to_string`) e `impl Display for Node`, y el límite `Limits::max_text_size` (4 MiB), sin dependencias nuevas. El lexer es perezoso sobre `&str` (dos *tokens* de anticipación) y el parser, de precedencia ascendente (*precedence climbing*), construye un árbol con posiciones que después pasa por los constructores de `Node`, así que los errores de construcción son los de JSON. Cada nodo guarda su altura y el lector se detiene en cuanto uno supera `max_depth`, así que nunca guarda un árbol más profundo que el límite. Cota de anidamiento (paréntesis, `NOT`, `-`, `ABS`) `2 · max_depth + 2`: en *release*, el peor texto con los límites por defecto necesita menos de 640 KiB de pila (las funciones hoja van `#[inline(never)]`); en *debug*, varias veces más.
  - 73 vectores nuevos al principio (`conformance/vectors/format/text.json`; campos aditivos `given.rules_text`, `given.write` y `expect.printed`), escritos a mano, que pasan en el núcleo y en Python; con la precedencia de `AND` alterada fallan 17. Propiedad nueva (`conditions_round_trip_through_text`, con claves, cadenas y números raros) y su equivalente con Hypothesis en Python; con el escape de comillas invertidas roto, falla. Cobertura de `text.rs` (con `cargo llvm-cov`, ya instalado en local): 98,4 % de líneas, 100 % de funciones.
  - Python: `format="text"` en `loads`, `load`, `dumps` y `dump` (sufijo `.rules`; los ficheros de texto acaban en salto de línea) y `cleanrules.rule_from_text(text, *, do=None)`, que llama a `text::parse_rule` del núcleo, devuelve un `Rule` y admite una acción porque el texto no lleva código. Ejemplo en `docs/examples/discounts.rules`, que un test mantiene canónico.
  - Desviaciones: los valores del *payload* se leen como expresiones completas para que `THEN late = days > 30` dé el error de JSON (`rule_definition_error`, en la posición del valor) y no uno de sintaxis; el lector aplana `and`/`or` anidados y ordena los valores de `in`, como el núcleo. El error de profundidad se sitúa en el primer nodo completado que la supera, no en el primero en preorden como en JSON, porque el texto se lee de abajo arriba.
  - Revisión de la API (subagente `api-reviewer`), aplicada:
    - Bloqueante: una cadena aritmética larga (`x+x+…`, 600 KB) abortaba el proceso por desbordamiento de pila al soltar el árbol (y en Python, *segfault*): la cadena se construye en un bucle, fuera de la cota de recursión. Ahora cada nodo comprueba su altura al crearse; un test lee cadenas de un millón de términos en un hilo de 2 MiB.
    - Memoria: un texto de 16 MiB (`a|a|…`, 8,4 millones de nodos) llegaba a 2,5 GB; con el lexer perezoso, 1,7 GB. Como con YAML, el texto tiene su propio límite, 4 MiB (448 MB en el peor caso, frente a 503 MB de JSON a 16 MiB).
    - Claves del *payload* repetidas: `rule_definition_error` en la segunda (JSON ya las rechaza). Los errores de un valor del *payload* o de la condición se sitúan en él.
    - `2 · max_depth + 2` usa aritmética saturada; la cota está en la especificación y su mensaje da el límite y una pista.
    - `spec/errors.md`: `limit_exceeded` y `registry_error` dan línea y columna desde texto.
    - `text::parse_rule` en el núcleo, para que ningún binding decida qué es una regla; los errores salen en el *token* donde empieza la regla o debería acabar el texto.
    - Sufijo `.rules`, en la especificación y en `load`/`dump`.
    - Tras un `.` cualquier identificador es una clave, palabras clave incluidas (`order.date`, `item.priority`), y se imprime así.
    - Menores: pistas para decimales y enteros fuera de rango; mensajes de `FLOAT`/`DECIMAL`/`DATE` con esas palabras y sin espacios alrededor; solo espacio, tabulador y saltos de línea como blancos (y una marca de orden de bytes al principio); sin ceros a la izquierda; sin caracteres de control en nombres ni en cadenas entre comillas simples (una clave con ellos no tiene texto: `unsupported`); constantes con `match` exhaustivo; `to_text` pasa a `to_string`, como `json` y `yaml`; pista sobre el orden de la cabecera; orden de los errores especificado; reservados `IS <término>` y `THEN … WITH <peso>` para la fase J.
    - 12 vectores nuevos (85 en total) y una propiedad nueva sobre reglas y rule sets con nombres, etiquetas, claves y descripciones raras.
- [x] D.2b Texto legible de `explain()` y `summary()` en el núcleo con la sintaxis canónica, *snapshots* con `insta` y `str()` en Python, también de las condiciones (`str(expr)` da hoy el `repr` de Python; fuzzytool muestra sus reglas como texto).
  - Hecho el 2026-11-02. Decisión del autor: los floats del texto canónico se escriben con dígitos normales y `e0` cuando su magnitud es 0 o está en [1e-4, 1e16) (`0.7e0`, `42500e0`) y con exponente fuera (`1e-7`), en lugar de `7e-1`; los datos y constantes de Python son floats y aparecen en casi todas las explicaciones. Cambia la impresión de D.2a (un vector actualizado y uno nuevo, `format/text/float-printing`).
  - Núcleo: módulo `explain::text` con `Display` para `Explanation`, `RuleExplanation`, `Cause` y `Summary`. Las condiciones, rutas, nombres y constantes salen como en el texto canónico; los valores de los datos, como constantes y acotados (cadenas de más de 60 caracteres, listas y mapas de más de 6 elementos, anidamiento de 2); los *gaps*, como números (floats a 12 cifras significativas, sin ruido), días o duraciones (`1 hour 30 minutes 0.5 seconds`). El formato sigue al de la v1 con la sintaxis nueva; `spec/explain.md` (*Text* y *Summaries*) lo describe con ejemplos que son salida real (un *snapshot* fija el del resumen).
  - Cambios de API: `RuleExplanation::rule` pasa a ser la regla (`&Rule`) y no su nombre, porque el texto necesita su condición y su descripción; `Summary::actions` (una por regla, `false` en el núcleo, que no tiene acciones; la ponen los bindings). El JSON del resumen escribe `"action": true`, como ya decía la especificación y no se hacía.
  - Tests: 9 *snapshots* con `insta` (dependencia de desarrollo aprobada en D.2a) que cubren todos los estados, notas, grupos anidados, conflictos, motivos, contrafactuales de cada tipo y resúmenes; tests unitarios de duraciones, *gaps*, motivos y valores acotados; propiedad nueva (`the_text_lists_every_rule_and_cause`). Cobertura de `explain/text.rs`: más del 96 % de líneas, 100 % de funciones.
  - Python: `str()` de `Explanation`, `RuleExplanation`, `Cause` y `Summary` da ese texto (los genera el núcleo), y `str(expr)` el texto canónico (`repr` sigue siendo Python). `RuleSummary.action`. El README muestra la explicación real y un test comprueba que no se desfasa.
  - Formato de la cabecera: `HIT POLICY priority: selected e` (`no rule` si no hay ninguna; con `AGGREGATE` si lo hay); los nombres en prosa van sin comillas cuando son identificadores, como en el texto. El texto de un rule set sin reglas dice `(no rules)`.
  - Revisión de la API (subagente `api-reviewer`), sin bloqueantes; aplicado:
    - Una clave con caracteres de control rompía el texto en varias líneas: al mostrarse (no en el texto canónico, que la rechaza) va con escapes de JSON entre comillas invertidas.
    - Acotados también las claves de los mapas, el nombre de tipo de los valores opacos y el detalle de los motivos (un `read_error` lleva el mensaje de la excepción de Python, que puede tener saltos de línea), con escapes, para que cada línea del texto sea una regla, una causa, una nota o una cabecera.
    - Python: el texto se genera la primera vez que se pide y se comparte entre la explicación, sus reglas y sus causas (`explain()` había pasado de 4,5 a 8 ms con 300 reglas al calcularlo siempre; vuelve a 4,5). El campo privado `_text` no entra en `__init__`, `__match_args__` ni la comparación, y un `Cause` o `RuleExplanation` hecho o cambiado a mano (`dataclasses.replace`) da su `repr`.
    - Núcleo: `#[non_exhaustive]` en los tipos de explicaciones y resúmenes, `Summary::with_actions` (siempre una entrada por regla), ejemplo en la documentación del módulo y nota de que el miembro JSON `rule` es el nombre.
    - Pendiente, a propósito: `str(rule)` y `str(ruleset)` siguen dando el `repr` (el texto no admite metadatos ni acciones); las líneas de la descripción no llevan marca propia; las columnas del resumen cuentan caracteres, no anchura visual (CJK, emoji). Vectores con el texto esperado de explicaciones, para los bindings futuros, cuando haya un segundo binding.
- D.3 Tablas de decisión con políticas DMN (la prioridad de los valores de salida se compila a prioridades de regla), desde filas, CSV o JSON. Se divide en dos commits:
- [x] D.3a Modelo, JSON y CSV en el núcleo, compilación a rule set, vectores y carga y volcado en Python.
  - Hecho el 2026-11-02. Decisiones del autor: las celdas usan la sintaxis de texto de D.2 sin su columna (no FEEL); la tabla es un modelo propio que se lee y escribe como tabla (JSON, YAML y CSV) y se compila a un rule set; se admite la política `output_order` (O de DMN), compilada; el CSV marca las columnas con las palabras de la sintaxis (`RULE`, `PRIORITY`, `DESCRIPTION`, `THEN clave [IN (valores)]`) y admite líneas de ajustes (`HIT POLICY`, `AGGREGATE`, `LOGIC`, `STRICT`).
  - Especificación nueva, `spec/tables.md`: modelo, celdas de entrada y salida, compilación (nombres `row_<n>`, condición como conjunción, prioridad de salida: con valores de salida, `k − r` bajo `priority` y orden estable bajo `output_order`, que compila a `rule_order`), JSON (contenido `"decision_table"`, aditivo en la versión 1, con su JSON Schema y 13 ejemplos), CSV (RFC 4180 estricto, separador detectado entre coma, punto y coma y tabulador, letras de DMN) y texto canónico (cadenas entre comillas simples, celdas sin columna).
  - Núcleo: `cleanrules::table` (`DecisionTable::new`, `from_csv`, `to_csv`, `ruleset`), lectura y escritura JSON en `format/table.rs` y `Document::DecisionTable`; el parser de texto gana celdas, expresiones y cabeceras de salida, y la impresión un estilo con comillas simples. Una tabla que no tendría texto canónico (celda que no es un predicado sobre su columna, clave con caracteres de control) se rechaza al construirla, así que escribir una tabla nunca falla. Sin dependencias nuevas: el lector de CSV es propio.
  - 86 vectores al principio (`conformance/vectors/format/tables.json`; campos aditivos `given.table_csv`, `given.write: "csv"`, `expect.csv` y `expect.compiled`), escritos a partir de la especificación, que pasan en el núcleo y en Python; con el rango de prioridad o el orden de `output_order` alterados fallan. Propiedad nueva: tablas con nombres, descripciones y celdas raras vuelven igual por CSV y por JSON (falla si el escritor de CSV no entrecomilla las comas). Cobertura: `table.rs` 97 %, `table/csv.rs` 97 %, `format/table.rs` 99 % de líneas.
  - Python: `DecisionTable` (con `ruleset`, `evaluate`, `inputs`, `outputs` y `rows`), `TableOutput`, `TableRow`; `format="csv"` y ficheros `.csv` en `load`/`dump`; tablas en JSON y YAML. Ejemplo canónico en `docs/examples/risk.csv`, que un test mantiene.
  - Revisión de la API (subagente `api-reviewer`), aplicada:
    - Bloqueantes. (1) Amplificación de memoria: cada celda copia la expresión de su columna (`Node` es un árbol de `Box`) y CSV rellena las filas cortas; un CSV de 24 KB llegaba a 11 GB. Ahora los lectores cuentan, mientras leen, una unidad por celda y los nodos de cada condición o valor, copias incluidas, con el límite nuevo `Limits::max_table_size` (2²¹); el peor caso medido, 479 MB, como JSON. El lector de CSV ya no guarda los registros: lee fila a fila y solo vuelve a leer el texto para situar un error de compilación. (2) Comprobaciones cuadráticas: nombres con `BTreeSet` (también en `RuleSet::new`), rango con búsqueda binaria y valores de salida ordenados (orden compatible con `any_equal`) para duplicados y búsquedas.
    - Orden de los errores como dice la especificación: columnas antes que filas (también en CSV), número de celdas y valores nulos en la forma, la lógica después de la forma; vectores con dos errores a la vez.
    - `AGGREGATE` antes de una letra de DMN que también lo da es un error; los valores de los ajustes no distinguen mayúsculas; el separador se busca en la primera línea que lo tiene; descripción vacía equivale a ninguna y nombre vacío es un error; los mensajes nombran la fila.
    - Rust: `TableOutput` y `TableRow` (los nombres de Python), `#[non_exhaustive]` con constructores (`TableRow::new(..).name(..)`), `TablePolicy` `#[non_exhaustive]`, ejemplo de `DecisionTable::new`.
    - Python: enumeración `TablePolicy`; `DecisionTable` se construye solo al leer (constructor privado para D.3b), no deja borrar atributos, se serializa con `pickle`, crea filas y rule set al primer uso y delega `evaluate_many` y `summary`; `load`/`dump` conservan los saltos de línea; CSV de algo que no es una tabla da `UnsupportedError`. El README muestra `docs/examples/risk.csv` y un test lo comprueba.
    - 13 vectores nuevos (99); la propiedad de ida y vuelta genera valores de salida, `\r`, nombres vacíos y agregados, y descarta con `prop_assume!`.
    - Pendiente, a propósito: compartir nodos (`Arc`) para no copiar la columna en cada celda sería un cambio del núcleo que va más allá de esta tarea.
  - Desviaciones: el CSV es RFC 4180 estricto, así que una celda o cabecera con comas (`IN ('a', 'b')`) va entre comillas en un fichero separado por comas, como hacen las hojas de cálculo; los nombres y descripciones entrecomillados conservan sus espacios. Bajo `priority`, filas con la misma prioridad de salida se resuelven por orden de fila.
- [x] D.3b Construcción de tablas en Python desde filas (y, de forma perezosa, desde un `DataFrame`), documentación y ejemplos.
  - Hecho el 2026-11-02. Decisión del autor: un `str` en cualquier celda es la sintaxis de texto, como en el CSV de la tabla (`"'VIP'"` es el texto VIP, `"VIP"` el campo `VIP`); cualquier otro valor es una constante. Así, filas de Python, un `DataFrame` y el fichero CSV construyen la misma tabla.
  - Desviación: el plan hablaba solo de Python, pero leer cabeceras y celdas es semántica, así que va al núcleo (invariante 2). Especificación nueva en `spec/tables.md` (*Grids*): cabecera y filas de celdas (texto leído como un campo CSV, valores como igualdad o constante, nodos construidos en el lenguaje anfitrión), con errores que apuntan a la rejilla (`/header/<c>`, `/rows/<r>/<c>`). Núcleo: `DecisionTable::from_grid`, `table::Header` y `table::Cell`; las filas llegan por un iterador y se leen de una en una, con el mismo presupuesto de tamaño que el CSV. La lectura de cabeceras CSV se comparte. Pista nueva cuando una celda de salida es un nombre suelto que coincide con uno de los valores de texto de su columna (`high` por `'high'`).
  - 28 vectores (`format/tables/grid-*`, entrada `table_grid`, aditiva, y `max_table_size` en los límites de los vectores). Pasan en el núcleo y en Python, que los construye con la API pública.
  - Python: `DecisionTable(header, rows, *, hit_policy, aggregate, logic, strict, max_depth, max_table_size)` y `DecisionTable.from_dataframe` (pandas o polars, sin importarlos; los valores ausentes son celdas vacías). `TableOutput.values` vale `()` por defecto. También se corrige un hueco de D.3a: `load` y `loads` aceptan `max_table_size`. polars no es dependencia de desarrollo, así que su camino se prueba con un objeto que imita su interfaz.
  - Revisión de API aplicada: un constructor incremental en el núcleo (`DecisionTable::grid` → `GridBuilder::push_row` → `finish`, con `from_grid` encima) que una C ABI puede exponer como *handles*; `HeaderItem` en Rust y en Python (exportado); conversiones `From` para celdas y cabeceras; `max_size` en los constructores de Python; la especificación y la documentación dicen que un `DataFrame` conserva los tipos de su biblioteca (pandas lee `0.15` como float), y `docs/pitfalls.md` explica las comillas de las celdas. Queda para J.4 un aviso de celdas que comparan con un campo que ninguna columna lee.
  - Ejemplo `docs/examples/risk_table.py`, probado: construye `risk.csv` desde filas y desde un `DataFrame`.
- [x] D.4 Acciones por nombre registrado en cada binding (nunca rutas de importación leídas de un fichero).
  - Hecho el 2026-11-02. Especificación: una regla puede nombrar su acción (`"do"` en JSON y YAML, `DO` en texto), que es un dato más de la regla: se lee, se escribe y aparece en los resúmenes (`"do"` junto a `"action": true`; `DO notify` en el texto del resumen), y el núcleo nunca la resuelve ni la ejecuta. Cada binding busca los nombres solo en un registro que da quien llama, nunca como rutas de importación (invariantes 9 y 12); un nombre desconocido es un `registry_error` con la regla, el nombre y los registrados, y `run` lo comprueba antes de evaluar, así que el error no depende de la entrada. Un rule set sin enlazar se evalúa, se explica y se escribe.
  - Cambio incompatible, aceptable porque nada está publicado: `DO` pasa a ser palabra clave del texto, así que un campo `do` se escribe `` `do` `` (vectores `do-is-a-keyword` y `do-as-a-field`). La clave `do`, que antes era un `format_error`, es ahora válida (adición al formato v1); el vector `format/rules/action-key` se sustituye.
  - Núcleo: `Rule::action_name` (un nombre vacío es `rule_definition_error`), `Rule::get_action_name` y `RuleSet::check_action_names`, que compara nombres y da el `registry_error` para que todos los bindings informen igual; lectura y escritura en JSON, YAML y texto; `Summary` marca las acciones con nombre y `with_actions` añade las de código. 16 vectores, que pasan en el núcleo y en Python.
  - Revisión de API aplicada: `action_name` para el nombre en todos los lenguajes (`do` no sirve de identificador en Scala ni en JS); la comprobación y su error en el núcleo; la especificación dice que un nombre no tiene estructura (ni módulo ni átomo: en Erlang, nada de `binary_to_atom`); un registro mal formado es `TypeError`, y `rule_from_text` no acepta `do` y `actions` a la vez.
  - Python: `Rule(do=...)` acepta una función o un nombre; `Rule.action_name` da el nombre y `Rule.do` el código (dado o enlazado); `RuleSet.bind(actions)` devuelve un rule set nuevo; `load`, `loads` y `rule_from_text` aceptan `actions=`, y `rule_from_text(do=...)` enlaza la función al nombre que da el texto. `dumps` escribe las acciones con nombre y solo rechaza el código sin nombre. El ejemplo `docs/examples/discounts.*` nombra la acción de `vip`.
  - Queda fuera: una columna `DO` en las tablas de decisión y el registro de una ejecución de `run` con sus acciones (`json-format.md` reserva su sitio); se harán cuando haga falta.
- [x] D.5 Cobertura del núcleo al 95 % de líneas (criterio de la *Definición de terminado*). Medida el 2026-11-01 con `cargo llvm-cov -p cleanrules --all-features`: 93,2 % en total, con `format/decision.rs` (70,1 %), `format/rules.rs` (84,0 %), `build.rs` (88,2 %), `json.rs` (91,5 %), `format/value.rs` (92,0 %), `value/datetime.rs` (92,7 %), `ast.rs` (93,8 %), `error.rs` (94,4 %) y `eval.rs` (94,7 %) por debajo. Lo observable desde fuera se cubre con vectores; lo interno, con tests unitarios; el código inalcanzable se elimina o se justifica. CI pasa a exigir el umbral.
  - Hecho el 2026-11-02. Cobertura de líneas de `crates/core` con `cargo llvm-cov -p cleanrules --features yaml`: del 94,3 % (tras D.4) al 97,9 %, y ningún fichero por debajo del 95 % (`format/decision.rs`, del 70,1 % al 98,8 %; `format/rules.rs`, al 100 %).
  - Vectores (107 nuevos, 2508 en total), escritos con el error esperado según la especificación y comparados con el núcleo antes de añadirlos: lectura de decisiones (71: la forma de la decisión, de los resultados, de los árboles y de los motivos, y cada una de las ocho comprobaciones, con decisiones válidas de `collect` con agregado, `between`, `not`, un campo ausente, `any` y un campo que no es booleano), la forma de reglas y rule sets (22), valores etiquetados mal formados (11), `abs` y `neg` del entero mínimo y un `$int` fuera de rango en los datos.
  - Tests unitarios para lo interno: constructores de `build` y el paso de errores, conversiones y `Display` de las celdas de la rejilla, el parser JSON (errores de sintaxis, escapes, UTF-8 inválido desde bytes, límite de tamaño), el `Hash` de `Node` (nodos iguales, mismo *hash*), `AwareDateTime` (orden, precisión, `Display`) y los accesores de `Evaluated`.
  - Código inalcanzable: en la lectura de decisiones, la constante de un resultado sin valor de verdad ya no tiene rama propia; al recalcular, el campo y la constante en posición de condición comparten rama (una constante ahí es booleana o nula). Lo que queda sin cubrir son sobre todo ramas defensivas que la construcción de los nodos hace inalcanzables (un nodo de valor evaluado como condición y al revés, la división entera y de floats, que se resuelve antes), la rama `None` comentada del lector JSON iterativo, una zona válida al leerla que no estuviera en la base de datos de zonas y los `panic!()` de los propios tests.
  - Cambio visible: los errores del modo estricto escriben la ruta como el texto (`items[2].price`, antes `items.[2].price`), con `text::path`, como ya hacían las explicaciones.
  - CI: trabajo `coverage` que ejecuta `cargo llvm-cov -p cleanrules --features yaml --fail-under-lines 95`.

---

### Fase E. C ABI y bindings C/C++ · tamaño L

- E.1 `crates/ffi`: handles opacos, propiedad de la memoria explícita, errores como códigos más mensaje, entrada y salida en JSON y en `Value` binario; cabecera con cbindgen; versionado de ABI. Se divide en dos commits (decisión del autor, 2026-11-03: primero JSON):
- [x] E.1a Handles, errores, versión de la ABI, documentos, rule sets, decisiones, explicaciones y resúmenes en JSON, YAML y texto; cabecera con cbindgen; Miri y sanitizers.
  - Hecho el 2026-11-03. Decisiones del autor: la cabecera la escribe la herramienta cbindgen (no una dependencia de compilación) y se versiona en el repositorio, y CI comprueba que está al día (`cbindgen --verify`, versión fijada); los errores salen por un parámetro `cr_error **error` opcional, al estilo de `GError`, y no por un "último error" del hilo.
  - Especificación: `spec/errors.md` define la forma C (`cr_status` de 32 bits: `CR_OK`, el número del código, `CR_INVALID_ARGUMENT` = -1 para un mal uso de la ABI y `CR_INTERNAL_ERROR` = -2 para un fallo nuestro), una tabla de números estables por código (*Numeric codes*) y la forma JSON de un error (*JSON form*: código, mensaje, pista, contexto y causa), que también usarán Go, la JVM y el servidor MCP. Un texto que no es UTF-8 es un `format_error` en el primer byte inválido, en todos los formatos.
  - Núcleo: `format::write_error`; `format::DocumentFormat` con `read_document_as` y `write_document_as` (leer de bytes y escribir en cualquier formato, con el error de CSV para lo que no es una tabla), `json::from_utf8` y `Limits::with_max_size`. Estaban repetidos en el binding de Python, que ahora los usa (invariante 2).
  - `crates/ffi` (crate `cleanrules-ffi`, biblioteca `cleanrules_ffi` estática y dinámica): `cr_document_read/kind/write`, `cr_ruleset_read/from_document/write/summary/evaluate/read_decision`, `cr_decision_write/output/outputs/explain`, `cr_limits_new/set/get`, `cr_error_code/message/hint/json`, `cr_status_name`, `cr_error_parent`, `cr_error_is`, `cr_string_free`, `cr_*_free`, `cr_abi_version`, `cr_version` y `cr_format_version`. Textos de entrada como puntero y longitud; de salida, terminados en NUL y con su longitud. Los handles son inmutables y se comparten entre hilos; una decisión guarda su rule set (`Arc`) y, como en Python, se rehace al leerla (el núcleo presta las decisiones de su rule set). Todo `unsafe` lleva `// SAFETY:`, toda función su `# Safety` (lints `undocumented_unsafe_blocks` y `missing_safety_doc` en `deny`), y los *panics* se capturan en la frontera.
  - Tests: 1149 vectores de conformidad a través de la ABI (documentos en todos los formatos, decisiones leídas, evaluaciones, explicaciones y resúmenes); 12 tests de la ABI (NULL, estados, errores, límites, propiedad, hilos) y uno de *panics*, que pasan también bajo Miri; el ejemplo `docs/examples/discounts.c` compila como C11 estricto y corre con ASan y UBSan; la cabecera compila como C++17. Cobertura de `crates/ffi`: 98,3 % de líneas. CI: trabajos `c-abi` (Linux y macOS) y `miri`.
  - Vectores: los tres de `registry_error` que no daban el `name`, comparado según `spec/errors.md`, ahora lo dan.
  - Revisión de la API (subagente `api-reviewer`), sin bloqueantes y sin nada inseguro; aplicado:
    - Decisión del autor: un documento con una regla da un rule set de esa regla con los ajustes por defecto (`spec/json-format.md`, *Documents*, *A rule set from a document*), en el núcleo (`Document::into_ruleset`); una condición es `format_error`. Antes, `cr_ruleset_read` rechazaba un fichero `.rules` de una sola regla.
    - Formatos en el núcleo (invariante 2): `parse_as`, `read_decision_document_as`, `write_decision_as`, `write_explanation_as` y `write_summary_as`; las decisiones solo en JSON o YAML (texto y CSV son `unsupported` al leer y al escribir); Python deja de tratar como JSON cualquier formato que no sea YAML al leer una decisión. La especificación dice qué documentos admite cada formato.
    - Límites: `CR_LIMIT_MAX_YAML_SIZE` y `CR_LIMIT_MAX_TEXT_SIZE` (texto y CSV); `CR_LIMIT_MAX_SIZE` fija los tres y lee el de JSON.
    - La forma JSON de un error es un contrato estable (solo se añaden miembros; `message`, `hint` y `cause` son texto libre), con JSON Schema (`spec/schemas/v1/error.schema.json`, 5 ejemplos); `write_error` desestructura todo el contexto, y los 395 errores que dan los vectores a través de la ABI validan contra él (un `node` puede ser la constante nula).
    - Contratos en la cabecera: quién libera cada texto (los de `cr_error_*` son del error; los de `cr_version` y `cr_status_name`, estáticos), `*error` debe ser `NULL` a la entrada y no se toca si todo va bien, las salidas son `NULL` tras un fallo y `cr_limits` sí cambia.
    - Cada `unsafe` rodea una sola operación con su `SAFETY:` (ningún cierre hereda un contexto `unsafe`); comprobación en compilación de que los handles son `Send + Sync`; los errores `CR_INVALID_ARGUMENT` nombran la función y dan una pista.
    - Menores: tipos `cr_format`, `cr_limit` y `cr_document_content`; `cr_status_name(CR_OK)` es `ok`; `cr_error_is` es cierto para un estado negativo igual; `cr_decision_outputs` en orden de reglas; `json::from_utf8` pasa a `utf8_text`; el ejemplo en C cierra sus fugas en los caminos de error.
    - Compilar el ejemplo encontró un choque de nombres en la cabecera (el tipo y la función `cr_document_kind`), corregido antes del commit; CI compila la cabecera como C y como C++.
  - Queda para E.1b: el constructor de valores nativos, acciones como *callbacks* registradas por nombre, rejillas de tablas, nodos sueltos y lotes.
- [x] E.1b Valores nativos (`cr_value`): constructores y lectores, evaluación sobre ellos, rutas que lee un rule set, y lectura de rule sets y decisiones sin JSON.
  - Hecho el 2026-11-03. Decisiones del autor: primero los valores (acciones, rejillas, nodos sueltos y lotes van a E.1c; Windows y `RuleSet.read_decision` en Python, a E.1d); un valor opaco es un nombre de tipo y un *token* que elige el programa, como en el núcleo y en Python: la biblioteca nunca guarda un puntero de C ni llama de vuelta. `spec/data-model.md` lo dice así (antes prometía *handles* con *callback* de liberación).
  - ABI: `cr_value_new_*` para cada clase del modelo de datos (nulos con motivo, con el mensaje de un `read_error`; decimales desde su texto, y los que no caben, nulos con `decimal_overflow`; cadenas UTF-8; fechas y fechas-hora con y sin zona, que conservan la zona IANA si la base de datos le da ese desplazamiento; listas cuyas posiciones sin fijar son nulas, mapas y registros; opacos), `cr_value_list_set` y `cr_value_map_insert` (el hijo pasa al padre aunque la llamada falle), `cr_value_kind`, `cr_value_get_*`, `cr_value_len`, `cr_value_list_get`, `cr_value_map_entry` y `cr_value_map_get` (prestados), `cr_value_from_json`, `cr_value_to_json` y `cr_value_free`. Un día imposible, o fuera de los años 1 a 9999, es `CR_INVALID_ARGUMENT`.
  - `cr_ruleset_evaluate_value` (la decisión guarda una copia de la entrada); `cr_ruleset_len`, `cr_ruleset_rule_name`, `cr_ruleset_hit_policy`, `cr_ruleset_aggregate`, `cr_ruleset_strict` y `cr_ruleset_field_paths`; `cr_decision_len`, `cr_decision_fired`, `cr_decision_truth`, `cr_decision_selected_len`, `cr_decision_selected`, `cr_decision_output_value` y `cr_decision_outputs_value`. Lo que dice la decisión se guarda al hacerla, como en Python, así que leerlo no vuelve a evaluar; escribirla y explicarla sí. Con esto la decisión propia del núcleo deja de hacer falta: se retoma si un benchmark lo pide.
  - Tests: todos los vectores de evaluación pasan también desde un valor nativo reconstruido pieza a pieza con los lectores y constructores; 10 tests de valores, también bajo Miri, que encontró una fuga en un test (no en la biblioteca). Cobertura de `crates/ffi`: 98,3 %. El ejemplo `docs/examples/discounts.c` construye la entrada en nativo y lee qué reglas dispararon y el descuento sin JSON.
  - Construir el ejemplo encontró otro choque de nombres en la cabecera (tipo y función `cr_value_kind`; el tipo pasa a `cr_kind`), que CI ya detecta al compilar la cabecera como C++.
  - Revisión de la API (subagente `api-reviewer`), aplicada:
    - Bloqueantes: el lector de fechas-hora con zona daba microsegundos negativos antes de 1970 (ahora redondea hacia abajo: segundos −1 y 500000), y `cr_value_new_list` con una longitud enorme abortaba el proceso (ahora devuelve `NULL`).
    - Valores que C no puede representar: una fecha fuera de los años 1 a 9999 o un desplazamiento de más de 18 horas son `CR_INVALID_ARGUMENT`, no un opaco con *token* 0, que en `ANY` sería igual al *token* 0 del programa; el programa decide (`spec/data-model.md`).
    - Zonas: `AwareDateTime::from_native` solo conserva la zona si el nombre coincide exactamente con el de la base de datos (antes `europe/madrid` se guardaba y escribía un JSON ilegible); también en Python.
    - Préstamos: el ítem que se mete en sí mismo se rechaza sin consumirlo; la cabecera dice hasta cuándo vale un puntero prestado y qué salidas son propias o prestadas; las salidas prestadas quedan a `NULL` tras un fallo, y una cadena vacía apunta a `""`, nunca a una dirección colgante.
    - Profundidad: los valores anidan como mucho 1024 listas y mapas (como JSON por defecto), porque liberarlos, copiarlos y evaluarlos es recursivo; `cr_ruleset_evaluate_value` recibe límites, como `cr_ruleset_evaluate`.
    - Nombres antes de fijar la ABI: `cr_value_type` (tipo; la función sigue siendo `cr_value_kind`), `CR_VALUE_NAIVE_DATETIME` con `cr_value_new_naive_datetime`, y los lectores `cr_value_get_date` y `cr_value_get_naive_datetime`; `CR_TRUTH_*`; `cr_ruleset_field_paths`, que da un `cr_value` en vez de JSON.
    - `cr_ruleset_aggregate` y `cr_ruleset_strict`; los errores de UTF-8 nombran la función y el argumento; `cr_ruleset_hit_policy` con `match` exhaustivo; el núcleo da `Decision::selected_positions`, que usan C y Python.
  - Fuera, a propósito: `why_not` de Python (la explicación lo cubre), las claves disponibles de un mapa para la pista de `missing_field`, una variante de evaluación que consuma la entrada en vez de copiarla, y el coste cuadrático de `Map::insert` y de `cr_value_map_entry` (a revisar con benchmark en E.1c, con los lotes).
- [ ] E.1c Acciones por nombre como *callbacks* de C (`run`), rejillas de tablas (`GridBuilder`), condiciones y valores sueltos, y lotes.
- [ ] E.1d CI de la ABI en Windows (bibliotecas del sistema con MSVC) y `RuleSet.read_decision` en Python, para que el concepto tenga el mismo nombre que `cr_ruleset_read_decision`.
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
  - Idea de la revisión de D.3b: avisar de una celda de tabla que compara con un campo de primer nivel que ninguna columna lee (`country = ES` por `'ES'`), el error más probable al escribir una tabla.

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

### Fase N. Erlang · tamaño M

**Objetivo**: usar cleanrules desde Erlang (y, por la BEAM, desde Elixir y Gleam) con la misma semántica y sin poner en riesgo la VM (D12).

- [ ] N.1 Especificación: correspondencia entre los términos de Erlang y `Value` en `spec/data-model.md`, con sus vectores. Cubre mapas con claves binarias o átomos, enteros grandes, flotantes, binarios UTF-8 como texto, los átomos `true`, `false`, `null` y `undefined`, fechas de `calendar` y decimales.
- [ ] N.2 NIF con `rustler` sobre `crates/core`.
  - `RuleSet` como recurso inmutable que comparten los procesos.
  - Una entrada se evalúa en el planificador normal; los lotes y las explicaciones van a *dirty schedulers* (o se trocean) para no pasar de ~1 ms por llamada.
  - Los *panics* se capturan y llegan como `{error, ...}`. Binarios precompilados por plataforma.
- [ ] N.3 API Erlang idiomática (módulo `cleanrules`).
  - Reglas construidas con términos y leídas de JSON (D.1); `explain/1` y `summary/1`.
  - Resultados `{ok, Decision} | {error, #{code := atom(), message := binary(), ...}}`.
  - Acciones como funs registradas por nombre (D.4), que ejecuta el proceso que llama. Especificaciones para Dialyzer.
- [ ] N.4 Runner de conformidad (Common Test), documentación con ex_doc y paquete en Hex con rebar3.

---

### Fase O. IA agéntica y frameworks de agentes · tamaño L

**Objetivo**: que los agentes de IA usen cleanrules como herramienta de decisión, como *guardrail* determinista y explicable de sus propias acciones, y como destino verificable de las reglas que proponen. El motor sigue sin ningún modelo dentro (D13).

- [ ] O.1 Especificación del contrato para agentes, con vectores.
  - Herramientas `evaluate`, `explain`, `summary`, `validate` y `list_rulesets`, con JSON Schemas de entrada y salida derivados del formato v1.
  - El esquema de entrada de cada rule set se genera desde `summary()` (los campos que lee y sus tipos).
  - Una explicación compacta para el contexto de un LLM, con un presupuesto de tamaño.
  - Errores con puntero, mensaje y pista que un modelo pueda corregir.
- [ ] O.2 Servidor MCP `cleanrules-mcp` (`crates/mcp`, binario).
  - Las herramientas de O.1 sobre rule sets cargados de un directorio configurado, que también se ofrecen como recursos MCP.
  - Transportes stdio y HTTP *streamable*.
  - Solo lectura por defecto, nunca ejecuta acciones, y con límites de tamaño y de tiempo.
  - Tests: la salida de cada herramienta coincide con la del núcleo.
- [ ] O.3 *Guardrails* para agentes.
  - Un rule set decide `allow`, `deny` o `ask` sobre una llamada a herramienta (nombre, argumentos y contexto) o sobre una salida, y devuelve al modelo la explicación y los contrafactuales.
  - El documento de decisión es el registro de auditoría: se relee y se verifica con las 8 comprobaciones.
  - Integración con los *hooks* del Claude Agent SDK (`PreToolUse`), los *guardrails* del OpenAI Agents SDK y los nodos e interrupciones de LangGraph.
- [ ] O.4 Adaptadores finos y opcionales (extras de carga perezosa) que convierten un `RuleSet` en herramienta de cada framework, probados contra versiones fijadas.
  - Python: Claude Agent SDK, OpenAI Agents SDK, LangChain/LangGraph, LlamaIndex, Pydantic AI y CrewAI.
  - JavaScript: Vercel AI SDK, LangChain.js y Mastra.
  - JVM: LangChain4j y Spring AI.
  - Go y Erlang, por MCP.
- [ ] O.5 Reglas escritas por agentes.
  - Ciclo proponer → validar (esquema y construcción, con errores corregibles) → auditar (J.4) → probar con casos → aprobación humana → desplegar, como herramientas MCP y como API.
  - Nunca `eval`: las reglas propuestas son datos (invariante 9).
  - Conjunto de evaluación (políticas en lenguaje natural → reglas) con métricas de validez y de acierto sobre casos reservados.
- [ ] O.6 Sistema experto para agentes (tras K): hechos que aportan las herramientas del agente, encadenamiento hacia delante y el árbol de derivación como explicación.
- [ ] O.7 Seguridad y documentación: modelo de amenazas (inyección de *prompts* a través de los datos, rule sets de origen no confiable, abuso de recursos), una guía por framework y ejemplos en `docs/`.
- [ ] O.8 Integración con swarmstate (backend de estado y *checkpoints* para agentes, con núcleo en Rust, del mismo autor; [swarmstate.github.io](https://swarmstate.github.io)), como extra opcional `cleanrules[swarmstate]` de carga perezosa. Nada de esto entra en el núcleo (invariantes 11 y 12). Revisado el 2026-11-02 sobre swarmstate 0.12.0.
  - Enrutado explicable. swarmstate enruta con `HandoffGraph` y condiciones en un minilenguaje propio, de dos valores (un tipo incompatible es `false`) y sin explicación. Un `RuleSet` (política `first`, payload con el destino) hace de router de `HandoffGraph` y de los `add_conditional_edges` de LangGraph, y devuelve por qué se eligió un destino y por qué no los demás. Incluye un conversor de las condiciones de `HandoffGraph` (`category in ['billing'] and user.tier == 'gold'`) a la sintaxis de texto, con la diferencia semántica documentada y probada: en cleanrules un tipo incompatible es `UNKNOWN`, también bajo `not`, y la regla no se dispara.
  - Registro de auditoría duradero: los documentos de decisión y de explicación de O.3 se guardan en un `Store` de swarmstate por hilo y paso, de modo que la vuelta atrás de swarmstate (instantáneas en O(1)) muestra también qué decidió el *guardrail* y por qué.
  - Validación del estado: un rule set en modo estricto comprueba el estado antes de que el *checkpointer* de swarmstate lo guarde, o comprueba el `diff` entre dos instantáneas, y rechaza el paso con la explicación y los contrafactuales.
  - Tests contra una versión fijada de swarmstate y ejemplo con LangGraph en `docs/`.
  - Idea para valorar con K y O.6: la memoria de trabajo del encadenamiento hacia delante sobre instantáneas de swarmstate, para probar hipótesis y volver atrás sin copiar el estado.

---

### Fase V. Visualización · tamaño M

**Objetivo**: ver condiciones, reglas, tablas, decisiones y explicaciones como diagramas, con el mismo resultado en todos los lenguajes (D14). Propuesta aceptada por el autor el 2026-11-02; V.1 y V.2 pueden empezar en cuanto termine D.3b.

- [ ] V.1 Especificación (`spec/visual.md`) y salida en el núcleo, con *snapshots* de `insta` y vectores.
  - Condición: el árbol del AST (`all`, `any`, `not` y las comparaciones), con cada hoja en la sintaxis de texto de D.2a.
  - Explicación: el árbol de causas de cada regla, con cada condición marcada por su valor de verdad (verdadero, falso o desconocido; el grado en lógica difusa) y sus contrafactuales, y la regla elegida resaltada.
  - Rule set: un nodo por regla con los campos que lee y sus salidas (los datos de `summary()`), y el orden o la prioridad de la política.
  - Tabla de decisión: la rejilla en HTML, con las filas que se disparan resaltadas cuando se le da una decisión.
  - Formatos: DOT (Graphviz), Mermaid (se ve en GitHub, GitLab y la documentación) y HTML autónomo, sin JavaScript ni recursos externos y con el texto escapado. Salida determinista y acotada como el texto de D.2b.
  - Un nodo nuevo del AST también se dibuja (invariante 3): se añade a la skill `/add-operator`.
- [ ] V.2 Python: `_repr_html_` para Jupyter en condiciones, reglas, rule sets, decisiones, explicaciones, resúmenes y tablas, con el HTML de V.1; `to_dot()`, `to_mermaid()` y `to_html()`; `to_graphviz()` con el paquete `graphviz` como extra opcional `cleanrules[viz]` de carga perezosa. Los demás bindings exponen `to_dot()`, `to_mermaid()` y `to_html()` a medida que existen.
- [ ] V.3 Estadísticas de un lote (tras I): cuántas veces se dispara cada regla y cuántas falla o queda desconocida cada condición, y la cobertura de un rule set o una tabla sobre un conjunto de datos. Las calcula el núcleo; se dibujan en HTML y, en Python, con matplotlib o plotly opcionales.
- [ ] V.4 *Playground* en la web (tras F): cleanrules compilado a wasm evalúa y dibuja reglas, tablas y explicaciones en el navegador, sin servidor.

---

### Fase M. Documentación y publicación 1.0 · tamaño M

- [ ] M.0 Documentación con Zensical (Material for MkDocs termina su soporte el 5 de mayo de 2027).
- [ ] M.1 Guías por lenguaje (Erlang incluido) y por framework de agentes con los mismos ejemplos, referencia de la especificación, matriz de funcionalidades y benchmarks reproducibles.
- [ ] M.2 Publicación sincronizada (crates.io, PyPI, npm, Maven Central, módulo Go, Hex, Conan y vcpkg, y el servidor MCP en el registro de MCP) con *trusted publishing* donde exista; política de estabilidad de la API y de la ABI.
- [ ] M.3 Paper en JOSS, DOI en Zenodo y `CITATION.cff`.
  - `CITATION.cff` añadido por adelantado el 2026-10-31, sin `version` ni `doi`. Falta añadirlos con la primera *release*, junto con el DOI de Zenodo y el paper.

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
P (hecha) → 0' → A → B → C ─┬→ D → E → F → G → H → N
                            ├→ I (necesita B; integra con C, F, G)
                            ├→ J (necesita B y D)
                            ├→ K (necesita B y D)
                            ├→ L (necesita C)
                            ├→ O (necesita B, C y D; los adaptadores llegan a cada binding cuando existe)
                            └→ V (V.1 y V.2 tras D.3b; V.3 tras I; V.4 tras F)
todas → M
```

Los bindings E–H y N siguen el orden decidido (C/C++ → JS → JVM → Go → Erlang); N necesita el núcleo y D.4, no la C ABI. O.1–O.3 pueden empezar en cuanto termine D, porque el servidor MCP solo necesita el núcleo y la carga de reglas; O.5 usa la auditoría de J.4, y O.6, el sistema experto de K. Las letras N y O se añadieron en la v3 del plan, y V el 2026-11-02; todas van antes de M. Las fases de funcionalidad I–K se implementan en el núcleo y llegan a todos los bindings existentes.

## 6. Riesgos

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Ocho ecosistemas que empaquetar y mantener | Alto | Bindings finos, C ABI común, conformidad compartida y CI única desde E.1 |
| Divergencias entre lenguajes | Alto | Sin lógica en los bindings; vectores obligatorios |
| Perder conocimiento al retirar Python | Medio | Fase A antes de borrar; tag en git |
| Fallos de memoria en la frontera FFI | Alto | Handles opacos, propiedad explícita, sanitizers y `miri` |
| Coste de convertir datos entre lenguajes | Medio | `Value` y lotes Arrow; benchmarks por binding |
| Lógica difusa duplicada respecto a fuzzytool | Bajo | Solo las normas; tests contra fuzzytool como referencia |
| Un NIF que bloquea o falla tumba la VM de Erlang | Alto | rustler (captura *panics*), *dirty schedulers* para el trabajo largo, sin estado global, tests de carga |
| Los frameworks de agentes cambian deprisa | Medio | MCP como base estable; adaptadores finos, opcionales y probados contra versiones fijadas |
| Reglas propuestas por un LLM incorrectas o maliciosas | Alto | Las reglas son datos (sin `eval`); validación, auditoría estática, casos de prueba y aprobación humana antes de desplegar |
| Inyección de *prompts* a través de los datos evaluados | Medio | Los datos nunca son instrucciones para el motor; el servidor MCP es de solo lectura y no ejecuta acciones |
