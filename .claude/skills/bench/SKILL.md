---
name: bench
description: Run cleanrules benchmarks (criterion suites of the Rust core — evaluation, batch/Arrow validation, explanation, Miss Manners/Waltz expert-system suites — plus per-binding overhead and ML extraction in Python) and compare against the stored baseline and competitor libraries. Use before claiming a performance improvement, after touching hot paths or a binding's data conversion, or when updating the public benchmark page.
argument-hint: "[suite: eval|validation|explain|expert|binding|ml|all] [--compare]"
---

# Benchmarks

Argumentos: `$ARGUMENTS` (por defecto `all`).

1. Comprueba que existen los benchmarks de la suite: `crates/core/benches/` (criterion, desde B.10) y `benchmarks/` (bindings y rivales). Si no existen todavía, díselo al usuario y para.
2. Anota la máquina (`uname -a`, CPU y RAM), `rustc --version`, el perfil de compilación y las versiones de los rivales y de cada binding medido.
3. Ejecuta:
   - núcleo: `cargo bench -p cleanrules --bench <suite> -- --save-baseline <fecha>`;
   - `binding`: el coste de cruzar la frontera por fila y por lote en cada binding existente, con su herramienta (pytest-benchmark, vitest bench, JMH, `go test -bench`);
   - `--compare`: los rivales según la suite, entre ellos zen-engine (Rust, Node, Python), Drools y CLIPS (expert), rule-engine, experta, durable_rules y clipspy (Python), pandera (validation) e imodels (ml).
   Guarda los resultados en `benchmarks/results/<fecha>-<suite>.json`.
4. Compara con la línea base (`criterion --baseline` y `benchmarks/results/baseline.json`):
   - una regresión de más del 10 % en cualquier caso se marca como **regresión** y se investiga antes de seguir;
   - una mejora se muestra como factor (×), con mediana y rango intercuartílico. Nunca se reporta una sola ejecución.
5. Presenta una tabla con caso, cleanrules, rival, factor y nota. No afirmes que cleanrules es "más rápido" si los intervalos se solapan.
6. Actualiza la línea base solo si el usuario lo pide explícitamente.
