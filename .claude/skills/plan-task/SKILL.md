---
name: plan-task
description: Implement one task from docs/PLAN.md end to end (tests first, implementation, quality gates, docs, checkbox). Use when the user says "siguiente tarea", "implementa la fase N", "/plan-task 2.3", or asks to continue the RulePy roadmap.
argument-hint: "[fase.tarea]  (vacío = siguiente tarea sin marcar)"
---

# Implementar una tarea del plan

Argumento: `$ARGUMENTS` (por ejemplo `2.3`). Si está vacío, toma la primera casilla `- [ ]` de la fase más baja sin terminar de `docs/PLAN.md`.

## 1. Situarse
- Lee `CLAUDE.md`, la fase completa en `docs/PLAN.md` (objetivo, entregables, criterios de aceptación y riesgos) y las especificaciones de `docs/spec/` que afecten a la tarea.
- Si la tarea depende de otra que no está marcada, dilo y propón hacer primero la dependencia.
- Si la tarea es demasiado grande para un único commit revisable, divídela en subtareas dentro de `docs/PLAN.md` y haz solo la primera.

## 2. Especificar
- Si la tarea define o cambia semántica (operadores, nulos, políticas, formato), actualiza primero `docs/spec/` y sube `rulepy_ast` cuando cambie el formato serializado.

## 3. Tests primero
- Escribe tests que fallen: unitarios en `tests/` reflejando `src/rulepy/`, propiedades con hypothesis (reutiliza `tests/strategies.py`) y snapshots de `explain()` cuando haya salida textual.
- Ejecuta `uv run pytest <ruta>` y confirma que fallan por la razón esperada.

## 4. Implementar
- Respeta los invariantes de `CLAUDE.md`. Si añades un nodo al AST, sigue `/add-operator`.
- Implementación clara antes que rápida. La optimización corresponde a la fase 7.

## 5. Puertas de calidad
```bash
uv run pytest -m "not slow"
uv run ruff check . && uv run pyright
cargo clippy --workspace -- -D warnings && cargo test --workspace   # solo si se tocó crates/
```
- Si cambió algo con más de un backend, lanza el subagente `parity-tester`.
- Si cambió la API pública, lanza el subagente `api-reviewer` y aplica los hallazgos de nivel *blocker* y *should-fix*.

## 6. Cerrar
- Docstrings, `CHANGELOG.md` (sección *Unreleased*) y un ejemplo en `docs/` si es visible para el usuario.
- Marca la casilla en `docs/PLAN.md` y anota debajo cualquier desviación o decisión tomada.
- Propón un mensaje de commit (Conventional Commits). No hagas commit ni push salvo que el usuario lo pida.
- Resume para el usuario: qué se hizo, el resultado de los tests y lo que queda pendiente o se ha decidido.
