---
name: add-operator
description: Add a new node/operator to cleanrules's condition AST (e.g. between, matches, is_null, startswith, arithmetic, aggregations) and wire it through the spec, the conformance vectors, every part of the Rust core and every existing binding so invariant 3 holds. Use whenever a new kind of condition or expression is needed.
argument-hint: "<nombre-del-operador> [semántica breve]"
---

# Añadir un operador al AST

Operador: `$ARGUMENTS`

Un operador no está terminado hasta que todos los puntos de esta lista estén hechos o devuelvan explícitamente un error `Unsupported` con un motivo. Lleva la lista como checklist en tu respuesta. Si una parte del núcleo o un binding todavía no existe según `docs/PLAN.md`, márcala como "no aplica aún".

1. **Especificación** (`spec/`): la forma JSON del nodo y su semántica con valores ausentes y nulos (Kleene), con tipos mixtos (int, float, decimal, texto, fechas), con desbordamiento y en lógica difusa. Actualiza el JSON Schema, y sube la versión del formato si el cambio no es aditivo.
2. **Vectores** (`conformance/`): construcción válida e inválida, evaluación con sus motivos, modo estricto, explicación y resumen, y nulos. Usa el subagente `conformance-tester`.
3. **Núcleo** (`crates/core`):
   - la variante del AST con serde, sus validaciones al construir, la identidad estructural, `field_paths` y `Display`;
   - el evaluador, que pasa por `Logic` cuando produce un valor de verdad y devuelve un `Reason` cuando no puede evaluar;
   - la explicación (requisito, texto y contrafactual cuando tenga sentido, por ejemplo "fallaría si x <= 5") y el resumen;
   - la evaluación Arrow por lotes, con la misma semántica de nulos;
   - la auditoría (abstracción por intervalos o conjuntos; si no es abstraíble, se trata como desconocido, que es lo correcto);
   - Rete, si el nodo puede aparecer en patrones;
   - el DSL de texto (sintaxis y precedencia), si existe;
   - los diagramas (`crates/core/src/visual`, `spec/visual.md`): un conector lógico nuevo (como `and`, `or`, `not`) es un nodo `connective` con su etiqueta y una arista a cada operando en `Diagram::condition`, y en la especificación; cualquier otro nodo es una hoja con su texto canónico, que no necesita código, pero sí un vector `draw` en `conformance/vectors/visual/` que lo muestre (con DOT, Mermaid y HTML si su texto tiene comillas, `<`, `&`, `#`, `` ` `` o `\`). Si el nodo cambia el árbol de causas de las explicaciones, revisa también `model::explanation`.
4. **Bindings**: el constructor idiomático en cada binding existente (Python, C/C++, JS, JVM, Go), con sus tipos y documentación. Ninguno evalúa el nodo por su cuenta.
5. **Tests**: unitarios y `proptest` en el núcleo, *snapshots* de `explain()` y de los diagramas (`crates/core/tests/visual.rs`), ida y vuelta JSON/YAML/DSL, y el runner de conformidad de cada binding en verde.
6. **Documentación**: la tabla de operadores en `docs/`, ejemplos por lenguaje y la entrada en `CHANGELOG.md`. Lanza el subagente `api-reviewer`.

Al terminar, ejecuta las puertas de calidad de `/plan-task` (paso 5).
