#!/bin/bash
# codex-meter.sh: pass-through `codex` that appends one JSON line per call
# (time, duration, exit code, model, tokens) to the campaign usage log.
# Set it as llm.agents.codex.executable; stdin, stdout and stderr pass through.
#
# Tokens come from the `turn.completed` event of `--json` runs on stdout
# (input, cached input and output tokens; `tokens` is input plus output) or,
# without `--json`, from the `tokens used` line on stderr. A call that reports
# neither logs `"tokens":null`.
[ "$1" = "--version" ] && exec "${CODEX_REAL:-$(command -v codex)}" "$@"
REAL=${CODEX_REAL:-$(command -v codex)}
LOG=${CODEX_METER_LOG:-${XDG_STATE_HOME:-$HOME/.local/state}/linguist-anki-bridge/campaign/codex-usage.jsonl}
err=$(mktemp)
out=$(mktemp)
start=$(date +%s%N)
"$REAL" "$@" > "$out" 2> "$err"
code=$?
end=$(date +%s%N)
cat "$out"
cat "$err" >&2
usage=$(grep -o '"type":"turn.completed","usage":{[^}]*}' "$out" | tail -1)
tokens=null
extra=""
if [ -n "$usage" ]; then
  field() { printf '%s' "$usage" | grep -o "\"$1\":[0-9]*" | head -1 | cut -d: -f2; }
  in=$(field input_tokens); cached=$(field cached_input_tokens); outp=$(field output_tokens)
  tokens=$(( ${in:-0} + ${outp:-0} ))
  extra=$(printf ',"input":%d,"cached":%d,"output":%d' "${in:-0}" "${cached:-0}" "${outp:-0}")
else
  t=$(grep -A1 '^tokens used' "$err" | tail -1 | tr -d ', ')
  [ -n "$t" ] && tokens=$t
fi
model=""
prev=""
for a in "$@"; do [ "$prev" = "-m" ] && model=$a; prev=$a; done
ms=$(( (end - start) / 1000000 ))
printf '{"at":%s,"ms":%d,"exit":%d,"model":"%s","tokens":%s%s}\n' \
  "$(date +%s)" "$ms" "$code" "$model" "$tokens" "$extra" >> "$LOG"
rm -f "$err" "$out"
exit $code
