#!/usr/bin/env bash
# Named speed and provider presets for a running campaign (WP-24).
#
#   scripts/campaign-preset.sh list
#   scripts/campaign-preset.sh show
#   scripts/campaign-preset.sh PRESET [CAMPAIGN_ID] [--no-restart]
#
# A preset writes the drafting provider, its fallback, the five-hour window
# caps and the prepare workers into the configuration file (a timestamped
# backup first, then `config validate`). A campaign driver runs the settings
# it started with, so a running driver is wrapped up and started again; a
# preset with fixed limits then tunes prepare-ahead and prepare-parallel
# (the careful-mode ramp would otherwise climb from one batch). Run it from a
# Claude Code session so `start` can bind the resume waiter to it; the
# session's check cron (every 15 minutes, or every minute while a preset says
# so) is the agent's to change.
#
# Presets:
#   normal        Codex main, Claude Code fallback, both capped at 65%, 6 workers,
#                 limits from the careful-mode ramp (the standing policy)
#   normal-rapid  Codex main, Claude Code fallback, both capped at 65%, 10 workers,
#                 prepare-ahead 24 / parallel 8
#   codex-only    Codex only, no fallback (Claude Code never drafts), cap 65%, 6
#                 workers, ramp; the campaign waits at the Codex cap
#   codex-rapid   Codex only, no fallback, cap 65%, 10 workers, prepare-ahead 24 /
#                 parallel 8 (top speed within the Codex cap)
#   codex-full    As codex-rapid but Codex cap 100% (temporary, until told to revert).
#   claude-main   Claude Code main, Codex fallback, both 65%, ramp
#   claude-rapid  Claude Code main, Codex fallback, Claude Code 100%, Codex 65%,
#                 10 workers, prepare-ahead 24 / parallel 8
#   all-out       Claude Code main, Codex fallback, both 100%, 10 workers,
#                 prepare-ahead 24 / parallel 8 (short bursts: the caps come fast)
#
# The preset in force is kept in session/preset.
set -u
cd "$(dirname "$0")/.."
REPO=$PWD
if [ -n "${LINGUIST_BIN:-}" ]; then B=$LINGUIST_BIN
elif [ -x target/lab/linguist-anki-bridge-head ]; then B=$REPO/target/lab/linguist-anki-bridge-head
else B=$(command -v linguist-anki-bridge) || { echo "linguist-anki-bridge not found" >&2; exit 1; }
fi
STATE=${XDG_STATE_HOME:-$HOME/.local/state}/linguist-anki-bridge/campaign
SESSION=$STATE/session
mkdir -p "$SESSION"
CONFIG=${LAB_CONFIG:-${XDG_CONFIG_HOME:-$HOME/.config}/linguist-anki-bridge/config.toml}
KEY_FILE=${XDG_CONFIG_HOME:-$HOME/.config}/linguist-anki-bridge/ankiconnect-api-key
[ -n "${ANKICONNECT_API_KEY:-}" ] || { [ -r "$KEY_FILE" ] && export ANKICONNECT_API_KEY="$(cat "$KEY_FILE")"; }

# name|provider|fallback ("none" = []) |codex cap|claude cap|workers|prepare-ahead|prepare-parallel ("-" = ramp)
PRESETS="normal|codex|claude_code|65|65|6|-|-
normal-rapid|codex|claude_code|65|65|10|24|8
codex-only|codex|none|65|65|6|-|-
codex-rapid|codex|none|65|65|10|24|8
codex-full|codex|none|100|65|10|24|8
claude-main|claude_code|codex|65|65|6|-|-
claude-rapid|claude_code|codex|65|100|10|24|8
all-out|claude_code|codex|100|100|10|24|8"

preset_line() { printf '%s\n' "$PRESETS" | awk -F'|' -v n="$1" '$1==n'; }
cfg() { grep -E "^\"$1\" *=" "$CONFIG" | head -n1 | sed 's/^[^=]*= *//'; }

list() {
  printf '%-13s %-12s %-12s %-6s %-7s %-8s %s\n' preset provider fallback codex claude workers limits
  printf '%s\n' "$PRESETS" | awk -F'|' '{l=($7=="-")?"ramp":$7" / "$8; printf "%-13s %-12s %-12s %-6s %-7s %-8s %s\n",$1,$2,$3,$4"%",$5"%",$6,l}'
}

show() {
  echo "preset in force: $(cat "$SESSION/preset" 2>/dev/null || echo unknown)"
  for k in llm.provider llm.fallback llm.agents.codex.max_window_percent \
           llm.agents.claude_code.max_window_percent jobs.prepare_workers llm.agents.parallel; do
    printf '%-46s %s\n' "$k" "$(cfg "$k")"
  done
}

find_campaign() {
  "$B" --output json campaign list | python3 -c '
import json,sys
cs=json.load(sys.stdin)["campaigns"]
run=[c for c in cs if c["status"]=="running"]
rev=[c for c in cs if c["name"].startswith("Collection revamp") and c["status"]!="finished"]
pick=(run or rev or [None])[0]
print(pick["campaign"] if pick else "")'
}

set_key() {  # set_key KEY TOML-VALUE: replace an existing line, never add one
  local key=$1 value=$2
  grep -qE "^\"$key\" *=" "$CONFIG" || { echo "config has no $key line; set it once by hand" >&2; return 1; }
  sed -i -E "s|^(\"$key\" *= *).*|\\1$value|" "$CONFIG"
}

apply() {
  local name=$1 C=${2:-} restart=1 line
  [ "${3:-}" = --no-restart ] && restart=0
  [ "$C" = --no-restart ] && { C=""; restart=0; }
  line=$(preset_line "$name")
  [ -n "$line" ] || { echo "unknown preset $name" >&2; list >&2; return 2; }
  IFS='|' read -r _ provider fallback codex claude workers ahead parallel <<<"$line"
  [ -n "$C" ] || C=$(find_campaign)
  local backup="$CONFIG.bak-preset-$(date +%Y%m%d-%H%M%S)"
  cp "$CONFIG" "$backup"
  set_key llm.provider "\"$provider\"" \
    && set_key llm.fallback "$([ "$fallback" = none ] && echo '[]' || echo "[\"$fallback\"]")" \
    && set_key llm.agents.codex.max_window_percent "$codex" \
    && set_key llm.agents.claude_code.max_window_percent "$claude" \
    && set_key jobs.prepare_workers "$workers" \
    || { cp "$backup" "$CONFIG"; echo "config restored from $backup" >&2; return 1; }
  if ! "$B" --output json config validate >/dev/null 2>&1; then
    cp "$backup" "$CONFIG"; echo "config did not validate; restored from $backup" >&2; return 1
  fi
  echo "$name" > "$SESSION/preset"
  echo "preset $name written (backup $backup)"
  [ "$restart" = 1 ] && [ -n "$C" ] || { echo "no restart: a running driver keeps its old settings until wrap-up and start"; return 0; }
  if [ -f "$SESSION/driver.pid" ] && kill -0 "$(cat "$SESSION/driver.pid")" 2>/dev/null; then
    scripts/campaign-session.sh wrap-up "$C" | tail -n 2
    scripts/campaign-session.sh start "$C" | tail -n 3
  else
    echo "no driver runs; the next start uses $name"
  fi
  if [ "$ahead" != - ] && [ -f "$SESSION/driver.pid" ] && kill -0 "$(cat "$SESSION/driver.pid")" 2>/dev/null; then
    "$B" --output json campaign tune "$C" --prepare-ahead "$ahead" --prepare-parallel "$parallel" \
      | grep -E 'prepare_(ahead|parallel)'
  fi
  echo "now: restart the session's wait-change task and set the check cron (15 minutes; 1 minute only if asked)"
}

case "${1:-}" in
  list|"") list ;;
  show) show ;;
  -h|--help) sed -n '2,32p' "$0" ;;
  *) apply "$@" ;;
esac
