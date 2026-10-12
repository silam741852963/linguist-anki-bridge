#!/bin/bash
# campaign-agent.sh: one WP-24 agent turn, run by its own systemd user timer
# (independent of the tick, so batches waiting for judgment do not wait for
# the tick to finish).
#
# Only when `campaign status` says the agent is needed, and today's agent
# limit allows it, a headless Claude Code session runs the `campaign-tick`
# skill. While it runs, the campaign leaves `needs_agent` batches alone
# (`agent-slot` .. `agent-done`); afterwards an idle tick is started so the
# agent's decisions are picked up at once.
#
# Environment: LAB_CAMPAIGN (required), LAB_BIN (CLI), LAB_CLAUDE (claude).
set -u
C=${LAB_CAMPAIGN:?LAB_CAMPAIGN is required}
BIN=$(command -v "${LAB_BIN:-linguist-anki-bridge}")
[ -n "$BIN" ] || { echo "LAB_BIN not found" >&2; exit 1; }
CLAUDE=${LAB_CLAUDE:-claude}
STATE=${XDG_STATE_HOME:-$HOME/.local/state}/linguist-anki-bridge/campaign
mkdir -p "$STATE"
LOG="$STATE/ticks.log"
log() { echo "$(date -Is) $*" >> "$LOG"; }
KEY="$HOME/.config/linguist-anki-bridge/ankiconnect-api-key"
[ -z "${ANKICONNECT_API_KEY:-}" ] && [ -r "$KEY" ] && export ANKICONNECT_API_KEY=$(cat "$KEY")
field() { python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get(sys.argv[1]))' "$1"; }

# One agent at a time, whatever starts it.
exec 9> "$STATE/agent.lock"
flock -n 9 || exit 0

STATUS=$("$BIN" --output json campaign status "$C" 2>> "$LOG")
[ "$(echo "$STATUS" | field agent_needed)" = True ] || exit 0
if ! "$BIN" --output json campaign agent-slot "$C" > /dev/null 2>> "$LOG"; then
  log "agent limit reached for today"
  exit 0
fi
log "agent run"
# The agent gets the exact command to call (no variables): only these
# command prefixes are allowed, and the user-only ones are denied.
ALLOW=(Read Write)
DENY=()
for cmd in "campaign" "plans show" "plans resolve" "plans resolve-batch" "plans split-vocab" \
           "plans split-grammar" "plans edit" "plans regenerate" "jobs show" "jobs items"; do
  ALLOW+=("Bash($BIN --output json $cmd:*)")
done
for cmd in "campaign resume" "campaign close" "campaign cancel" "campaign tune" "jobs rollback" \
           "snapshots restore" "notes restore-merged" "notes delete-merged" "models"; do
  DENY+=("Bash($BIN --output json $cmd:*)")
done
"$CLAUDE" -p "/campaign-tick $C (CLI: $BIN --output json)" \
  --allowedTools "${ALLOW[@]}" --disallowedTools "${DENY[@]}" \
  >> "$STATE/agent-$(date +%F).log" 2>&1
log "agent exit $?"
"$BIN" --output json campaign agent-done "$C" > /dev/null 2>> "$LOG"
# Pick the decisions up now; a running tick sees them on its next look.
systemctl --user start --no-block linguist-campaign.service 2> /dev/null
exit 0
