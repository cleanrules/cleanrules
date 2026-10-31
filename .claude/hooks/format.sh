#!/usr/bin/env bash
# PostToolUse (Write|Edit): format the file Claude just wrote. Never blocks.
# .rs -> rustfmt ; .py -> sort imports + ruff format ; .go -> gofmt ;
# C/C++ -> clang-format and JS/TS -> the project's prettier, only if installed.
# Only import sorting is auto-fixed in Python: a full --fix would delete an
# import added one edit before the code that uses it.
f=$(jq -r '.tool_response.filePath // .tool_input.file_path // empty')
[ -n "$f" ] && [ -f "$f" ] || exit 0
root="${CLAUDE_PROJECT_DIR:-.}"

# Prefer the ruff pinned in uv.lock so the hook and CI format identically.
ruff="$root/.venv/bin/ruff"
[ -x "$ruff" ] || ruff="uvx ruff"

case "$f" in
  *.rs)
    rustfmt --edition 2024 "$f"
    ;;
  *.py | *.pyi)
    $ruff check --select I --fix --quiet --exit-zero "$f"
    $ruff format --quiet "$f"
    ;;
  *.go)
    command -v gofmt >/dev/null && gofmt -w "$f"
    ;;
  *.c | *.h | *.cc | *.cpp | *.hpp)
    command -v clang-format >/dev/null && clang-format -i "$f"
    ;;
  *.ts | *.tsx | *.js | *.mjs | *.cjs)
    prettier="$root/bindings/js/node_modules/.bin/prettier"
    [ -x "$prettier" ] && "$prettier" --write --log-level warn "$f"
    ;;
esac
exit 0
