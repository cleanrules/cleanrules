---
name: plan-task
description: Implement one task from docs/PLAN.md end to end (spec, vectors and tests first, implementation, quality gates, docs, checkbox). Use when the user says "siguiente tarea", "implementa la fase N", "/plan-task B.3", or asks to continue the cleanrules roadmap.
argument-hint: "[fase.tarea]  (vacío = siguiente tarea sin marcar)"
---

# Implementar una tarea del plan

Argumento: `$ARGUMENTS` (por ejemplo `B.3`). Si está vacío, toma la primera casilla `- [ ]` de la fase más temprana sin terminar de `docs/PLAN.md`, siguiendo el orden de la sección "Dependencias entre fases".

## 1. Situarse
- Lee `CLAUDE.md`, la fase completa en `docs/PLAN.md` (objetivo, tareas, criterios de aceptación y riesgos) y las secciones de la especificación (`spec/`, antes `docs/spec/`) que afecten a la tarea.
- Si la tarea depende de otra que no está marcada, o de una decisión pendiente del plan, dilo y propón resolverla primero.
- Si la tarea es demasiado grande para un único commit revisable, divídela en subtareas dentro de `docs/PLAN.md` y haz solo la primera.

## 2. Especificar
- Si la tarea define o cambia semántica (operadores, nulos, tipos, políticas, explicaciones, formato), actualiza primero `spec/` y los JSON Schemas, y sube la versión del formato si el cambio no es aditivo.

## 3. Vectores y tests primero
- Vectores de conformidad en `conformance/` para todo lo observable desde fuera (con el subagente `conformance-tester` si son muchos), citando la sección de la especificación.
- Tests unitarios junto al código en Rust, propiedades con `proptest` y *snapshots* con `insta` cuando haya texto (`explain()`, `summary()`).
- En un binding: los tests idiomáticos de ese lenguaje y su runner de conformidad.
- Ejecútalos y confirma que fallan por la razón esperada.

## 4. Implementar
- Respeta los invariantes de `CLAUDE.md`: la semántica va en `crates/core`, nunca en un binding. Si añades un nodo al AST, sigue `/add-operator`.
- Para trabajo extenso en Rust, delega en el subagente `rust-engineer`.
- Implementación clara antes que rápida; optimiza solo con benchmark (`/bench`).

## 5. Puertas de calidad
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace          # o cargo test --workspace
cargo test -p cleanrules --test conformance
```
- Los tests, linters y runner de conformidad de cada binding afectado (ver "Comandos" en `CLAUDE.md`).
- Mientras exista `src/rulepy` (hasta A.4), si se tocó: `uv run pytest -m "not slow" && uv run ruff check . && uv run pyright`.
- Si cambió la API pública de cualquier lenguaje o el formato JSON, lanza el subagente `api-reviewer` y aplica los hallazgos de nivel *blocker* y *should-fix*.

## 6. Cerrar
- Documentación, `CHANGELOG.md` (sección *Unreleased*) y un ejemplo en `docs/` si es visible para el usuario.
- Marca la casilla en `docs/PLAN.md` y anota debajo cualquier desviación o decisión tomada.
- Propón un mensaje de commit (Conventional Commits con ámbito: `core`, `ffi`, `py`, `js`, `spec`, `conformance`…). No hagas commit ni push salvo que el usuario lo pida.
- Resume para el usuario: qué se hizo, el resultado de los tests y lo que queda pendiente o se ha decidido.
