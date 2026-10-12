#!/bin/bash
# campaign-tick.sh: one WP-24 campaign tick, run by the systemd user timer.
#
# 1. `campaign run` advances every batch without judgment (no tokens): the
#    write lane and `prepare_parallel` prep workers loop on their own.
# 2. A paused campaign (pilot review, guardrail) notifies the user.
# The agent runs on its own timer (scripts/campaign-agent.sh).
#
# Environment: LAB_CAMPAIGN (required), LAB_BIN (CLI), LAB_MAX_SECONDS (tick
# budget), LAB_START_VOICEVOX=1, LAB_START_ANKI=1.
set -u
C=${LAB_CAMPAIGN:?LAB_CAMPAIGN is required}
BIN=$(command -v "${LAB_BIN:-linguist-anki-bridge}")
[ -n "$BIN" ] || { echo "LAB_BIN not found" >&2; exit 1; }
STATE=${XDG_STATE_HOME:-$HOME/.local/state}/linguist-anki-bridge/campaign
mkdir -p "$STATE"
LOG="$STATE/ticks.log"
log() { echo "$(date -Is) $*" >> "$LOG"; }
KEY="$HOME/.config/linguist-anki-bridge/ankiconnect-api-key"
[ -z "${ANKICONNECT_API_KEY:-}" ] && [ -r "$KEY" ] && export ANKICONNECT_API_KEY=$(cat "$KEY")

# Local services the pipeline needs; a missing one only makes batches wait.
# They run as their own transient user units: processes left in this
# oneshot service would be stopped with it.
launch() {
  local name=$1; shift
  if command -v systemd-run > /dev/null; then
    systemctl --user reset-failed "$name.service" 2> /dev/null
    systemd-run --user --unit="$name" --collect "$@" >> "$LOG" 2>&1
  else
    setsid nohup "$@" > "$STATE/$name.log" 2>&1 &
  fi
}
if [ "${LAB_START_VOICEVOX:-0}" = 1 ] && ! curl -fs -m 2 http://127.0.0.1:50021/version > /dev/null; then
  log "starting VOICEVOX"; launch linguist-voicevox /usr/lib/VOICEVOX/vv-engine/run
  sleep 20
fi
if [ "${LAB_START_ANKI:-0}" = 1 ] && ! curl -fs -m 2 http://127.0.0.1:8765 > /dev/null; then
  log "starting Anki"; launch linguist-anki /usr/bin/python3.14 /usr/bin/anki
  sleep 60
fi

run() {
  "$BIN" --output json campaign run "$C" --max-seconds "${LAB_MAX_SECONDS:-3000}" > "$STATE/run-last.json" 2>> "$LOG"
  log "campaign run exit $? stopped=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("stopped"))' "$STATE/run-last.json" 2>/dev/null)"
}
field() { python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get(sys.argv[1]))' "$1"; }

log "tick start"
run
STATUS=$("$BIN" --output json campaign status "$C" 2>> "$LOG")
if [ "$(echo "$STATUS" | field user_needed)" = True ]; then
  CODE=$(echo "$STATUS" | field code)
  MARK="$STATE/notified-$CODE"
  if [ ! -e "$MARK" ]; then
    command -v notify-send > /dev/null && notify-send "Linguist campaign paused" "$CODE: $(echo "$STATUS" | field detail)"
    touch "$MARK"
  fi
  log "paused: $CODE"
fi
log "tick end"
