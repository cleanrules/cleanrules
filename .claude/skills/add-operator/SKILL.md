---
name: add-operator
description: Add a new node/operator to RulePy's condition AST (e.g. between, matches, is_null, startswith, arithmetic, aggregations) and wire it through every backend so invariant 2 holds. Use whenever a new kind of condition or expression is needed.
argument-hint: "<nombre-del-operador> [semántica breve]"
---

# Añadir un operador al AST

Operador: `$ARGUMENTS`

Un operador no está terminado hasta que todos los puntos de esta lista estén hechos o lancen explícitamente `UnsupportedOperation` con un motivo. Lleva la lista como checklist en tu respuesta.

1. **Especificación**: en `docs/spec/ast.md`, documenta la forma JSON del nodo, su semántica con valores ausentes y nulos (Kleene), con tipos mixtos y en lógica difusa. Actualiza `ast-schema.json`, y sube la versión `rulepy_ast` si el cambio no es aditivo.
2. **Nodo** en `src/rulepy/core/expr.py`: una dataclass inmutable con igualdad estructural y su constructor público (método en `F`/`Field` o función).
3. **Evaluador de referencia** (`core/evaluate.py`): debe pasar por `Logic` cuando produce un valor de verdad.
4. **Traza y explicación** (`core/trace.py`): el valor de cada hijo, el texto para `explain()` y la pista contrafactual si tiene sentido ("fallaría si x <= 5").
5. **Serialización**: `to_dict`/`from_dict` y, si lo hay, la sintaxis en el DSL de texto (`io/text.py`) con su precedencia.
6. **Backend polars** (`validation/polars_backend.py`): la compilación a `pl.Expr` con la misma semántica de nulos.
7. **Auditoría** (`audit/`): la abstracción por intervalos o conjuntos y su traducción a z3. Si no es abstraíble, se trata como desconocido, que es lo correcto.
8. **Rust** (`crates/rulepy-core`), solo si ya existe la fase 7: la variante serde, la instrucción de bytecode y el soporte en Rete.
9. **Tests**:
   - unitarios por backend;
   - ida y vuelta de serialización;
   - paridad Python ↔ polars (↔ Rust), mediante el subagente `parity-tester`;
   - snapshot de `explain()`;
   - casos con nulos.
10. **Documentación**: la tabla de operadores en `docs/` y la entrada en `CHANGELOG.md`.

Al terminar, ejecuta las puertas de calidad de `/plan-task` (paso 5).
