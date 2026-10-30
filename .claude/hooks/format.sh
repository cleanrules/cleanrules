#!/usr/bin/env bash
# PostToolUse (Write|Edit): format the file Claude just wrote.
# .py -> sort imports + ruff format ; .rs -> rustfmt. Never blocks.
# Only import sorting is auto-fixed: a full --fix would delete an import added
# one edit before the code that uses it.
f=$(jq -r '.tool_response.filePath // .tool_input.file_path // empty')
[ -n "$f" ] && [ -f "$f" ] || exit 0

case "$f" in
  *.py)
    uvx ruff check --select I --fix --quiet --exit-zero "$f"
    uvx ruff format --quiet "$f"
    ;;
  *.rs)
    rustfmt --edition 2024 "$f"
    ;;
esac
exit 0
