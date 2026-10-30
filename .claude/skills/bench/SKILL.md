---
name: bench
description: Run RulePy benchmarks (batch evaluation, validation, Miss Manners/Waltz expert-system suites, ML extraction) and compare against the stored baseline and competitor libraries. Use before claiming a performance improvement, after touching hot paths, or when updating the public benchmark page.
argument-hint: "[suite: eval|validation|expert|ml|all] [--compare]"
---

# Benchmarks

Argumentos: `$ARGUMENTS` (por defecto `all`).

1. Comprueba que `benchmarks/run.py` existe (se crea en la fase 2). Si no existe, díselo al usuario y para.
2. Anota la máquina (`uname -a`, CPU y RAM), las versiones de Python, Rust y las dependencias, y el backend (`RULEPY_BACKEND=python|rust`).
3. Ejecuta `uv run python benchmarks/run.py --suite <suite> --json benchmarks/results/<fecha>-<suite>.json`. Si se pide `--compare`, incluye los rivales instalados en el extra `bench`: experta, durable_rules, clipspy, rule-engine, zen-engine, pandera e imodels, según la suite.
4. Compara con `benchmarks/results/baseline.json`:
   - una regresión de más del 10 % en cualquier caso se marca como **regresión** y se investiga antes de seguir;
   - una mejora se muestra como factor (×), con mediana y rango intercuartílico. Nunca se reporta una sola ejecución.
5. Presenta una tabla con caso, RulePy, rival, factor y nota. No afirmes que RulePy es "más rápido" si el intervalo se solapa.
6. Actualiza `baseline.json` solo si el usuario lo pide explícitamente.
