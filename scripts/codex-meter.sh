#!/bin/bash
# codex-meter.sh: pass-through `codex` that appends one JSON line per call
# (time, duration, exit code, model, tokens) to the campaign usage log.
# Set it as llm.agents.codex.executable; stdin, stdout and stderr pass through.
[ "$1" = "--version" ] && exec "${CODEX_REAL:-$(command -v codex)}" "$@"
REAL=${CODEX_REAL:-$(command -v codex)}
LOG=${CODEX_METER_LOG:-${XDG_STATE_HOME:-$HOME/.local/state}/linguist-anki-bridge/campaign/codex-usage.jsonl}
err=$(mktemp)
start=$(date +%s%N)
"$REAL" "$@" 2> "$err"
code=$?
end=$(date +%s%N)
cat "$err" >&2
tokens=$(grep -A1 '^tokens used' "$err" | tail -1 | tr -d ', ')
model=""
prev=""
for a in "$@"; do [ "$prev" = "-m" ] && model=$a; prev=$a; done
ms=$(( (end - start) / 1000000 ))
printf '{"at":%s,"ms":%d,"exit":%d,"model":"%s","tokens":%s}\n' \
  "$(date +%s)" "$ms" "$code" "$model" "${tokens:-null}" >> "$LOG"
rm -f "$err"
exit $code
