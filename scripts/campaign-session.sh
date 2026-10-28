#!/bin/bash
# campaign-session.sh: run a WP-24 campaign from an interactive session and
# end that session cleanly. Nothing here is a timer: the driver runs only
# between `start` and `wrap-up`, and Anki is never launched (it must already
# be open).
#
#   campaign-session.sh start [CAMPAIGN]    start VOICEVOX if needed, resume a
#                                           paused campaign, run the driver
#   campaign-session.sh watch [--seen] [CAMPAIGN]
#                                           return when the agent must act
#   campaign-session.sh status [CAMPAIGN]   driver, services, campaign status
#   campaign-session.sh wrap-up [CAMPAIGN]  stop the driver and watchers, let
#                                           the tick finish, end orphaned
#                                           agents, stop what start started,
#                                           record the handoff
#   campaign-session.sh heartbeat [CAMPAIGN]
#                                           the agent session is alive: its
#                                           15-minute check runs this (start
#                                           does too)
#   campaign-session.sh resume-when-ready [CAMPAIGN]
#                                           wait for the driver to wrap up and
#                                           no provider to cool down, then
#                                           start; only from a Claude Code
#                                           session, and it gives up once that
#                                           session closes; an agent session runs
#                                           it in its background after a
#                                           wrap-up, so the campaign resumes
#                                           only while that session lives
#
# The driver runs ticks back to back. A generation provider at its usage
# limit (or past llm.agents.max_calls_per_window, or past
# llm.agents.codex.max_window_percent of its own five-hour window) is skipped
# by the CLI while llm.fallback drafts. When no provider of the chain can
# draft (`campaign status` drafting_available false) and no batch is left to
# write (approved, applying, verifying or merging; a batch ready for the
# agent's approval does not hold the session), the session wraps up by itself: the campaign is paused for the
# next `start` (checked every two minutes during a tick and before each tick). A pause because the
# The driver is detached from the agent session that started it, so it also
# wraps up when that session stops checking in: no `heartbeat` for
# HEARTBEAT_SECONDS (a closed or dead session). `status` says when the driver
# runs an older copy of this script (restart it: wrap-up, then start). The
# full policy is in docs/cli/operations/campaign.md, "Session run policy".
# A pause because the
# local engine lost its certification (an upgraded Ollama) is certified
# again with `doctor --ollama --certify` and resumed when that passes. Any
# other stop ends the driver.
#
# Environment: LINGUIST_BIN (CLI; default target/lab/linguist-anki-bridge-head
# when present, else linguist-anki-bridge on PATH), PROBE_SECONDS (1800),
# TICK_SECONDS (3000), HEARTBEAT_SECONDS (2700; 0 turns the check off for a
# session the user runs by hand).
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
LOG=$SESSION/driver.log
USAGE=$STATE/codex-usage.jsonl
PROBE_SECONDS=${PROBE_SECONDS:-1800}
TICK_SECONDS=${TICK_SECONDS:-3000}
HEARTBEAT_SECONDS=${HEARTBEAT_SECONDS:-2700}
QUOTA_REASON="Codex usage limit reached; driver resumes when the quota resets"
WRAPPED="wrapped up at the end of a session"
KEY=$HOME/.config/linguist-anki-bridge/ankiconnect-api-key
[ -z "${ANKICONNECT_API_KEY:-}" ] && [ -r "$KEY" ] && export ANKICONNECT_API_KEY=$(cat "$KEY")

say() { echo "$(date '+%F %T') $*" | tee -a "$LOG"; }
cli() { "$B" --output json "$@"; }
json() { python3 -c "import json,sys;d=json.load(sys.stdin);print($1)"; }
setting() { cli config show 2>/dev/null | json "json.dumps(d.get('values',d).get('$1'))"; }
alive() { [ -f "$SESSION/$1.pid" ] && kill -0 "$(cat "$SESSION/$1.pid")" 2>/dev/null; }

campaign_id() {
  local ids
  if [ -n "${1:-}" ]; then echo "$1"; return; fi
  mapfile -t ids < <(cli campaign list | json '"\n".join(c["campaign"] for c in d["campaigns"] if c["status"] in ("running","paused"))')
  [ ${#ids[@]} = 1 ] || { echo "name the campaign: ${#ids[@]} are running or paused" >&2; return 1; }
  echo "${ids[0]}"
}
status_of() { cli campaign status "$1" 2>/dev/null | json 'd["status"]+"\n"+(d.get("detail") or "")'; }

# --- driver -----------------------------------------------------------------
probe() {  # 0 = Codex answers, 1 = usage limit, 2 = other failure
  local model o
  model=$(setting llm.agents.codex.model | tr -d '"')
  [ "$model" = null ] && model=""
  o=$(cd /tmp && timeout 180 codex exec ${model:+-m "$model"} --skip-git-repo-check "Reply with OK only." 2>&1)
  grep -qi "usage limit" <<<"$o" && return 1
  grep -qx "OK" <<<"$o" && return 0
  return 2
}
drafting_available() {  # false only when every provider of the chain cools down
  [ "$(cli campaign status "$1" | json 'd.get("drafting_available", True)')" != False ]
}
writes_pending() {  # batches that still reach Anki without drafting
  # Not `ready`: a ready batch waits for the agent's approval, which no tick
  # gives, so counting it kept an idle session from ever wrapping up.
  [ "$(cli campaign status "$1" | json 'sum(d["phases"].get(p, 0) for p in ("approved", "applying", "verifying", "merging"))')" != 0 ]
}
session_gone() {  # the agent session stopped checking in
  local beat
  [ "$HEARTBEAT_SECONDS" = 0 ] && return 1
  beat=$(stat -c %Y "$SESSION/heartbeat" 2>/dev/null || echo 0)
  [ $(( $(date +%s) - beat )) -gt "$HEARTBEAT_SECONDS" ]
}
time_to_wrap_up() {  # nothing can draft and nothing is left to write, or no session watches
  session_gone || { ! drafting_available "$1" && ! writes_pending "$1"; }
}
auto_wrap_up() {  # in its own process group: wrap-up ends the driver's group
  if session_gone; then say "no heartbeat from the agent session for over ${HEARTBEAT_SECONDS}s; wrapping up"
  else say "no provider can draft ($(cli campaign status "$1" | json 'd.get("agent_cooldowns")')); wrapping up"; fi
  setsid "$REPO/scripts/campaign-session.sh" wrap-up "$1" >> "$LOG" 2>&1 < /dev/null &
}
watchdog() {  # during a tick: wrap up once no provider can draft
  local C=$1
  while sleep 120; do
    time_to_wrap_up "$C" || continue
    auto_wrap_up "$C"
    return
  done
}
drive() {
  local C=$1 s fallback wd
  while [ ! -f "$SESSION/stop" ]; do
    mapfile -t s < <(status_of "$C")
    if [ "${s[0]:-}" != running ]; then
      if grep -q "Codex usage limit" <<<"${s[*]}"; then
        fallback=$(setting llm.fallback)
        if [ "$fallback" != "[]" ] && [ "$fallback" != null ]; then
          say "paused for quota; llm.fallback $fallback drafts meanwhile: resume"
        else
          say "paused for quota; probing every ${PROBE_SECONDS}s"
        fi
        until [ "$fallback" != "[]" ] && [ "$fallback" != null ] || probe; do
          sleep "$PROBE_SECONDS" & wait $!
          [ -f "$SESSION/stop" ] && return
        done
        say "requeue + resume"
        cli campaign requeue "$C" --reason AGENT: --reason APPLY_FAILED: --reason PREPARE_FAILED: >> "$LOG" 2>&1
        cli campaign resume "$C" >> "$LOG" 2>&1
        continue
      fi
      # An upgraded local engine: certify it again; resume only if it passes.
      if [ "$(cli campaign status "$C" | json 'd.get("code") or ""')" = GENERATION_ENGINE_UNVERIFIED ]; then
        say "local engine not certified; running doctor --ollama --certify"
        if cli doctor --ollama --certify > "$SESSION/certify-last.json" 2>> "$LOG" &&
          [ "$(json 'd.get("certification",d).get("passed")' < "$SESSION/certify-last.json")" = True ]; then
          say "certified; resume"
          cli campaign resume "$C" >> "$LOG" 2>&1
          continue
        fi
        say "certification failed: see $SESSION/certify-last.json"
      fi
      say "STOPPED ${s[*]}"; return
    fi
    if time_to_wrap_up "$C"; then auto_wrap_up "$C"; return; fi
    watchdog "$C" & wd=$!
    say "tick start"
    cli campaign run "$C" --max-seconds "$TICK_SECONDS" > "$SESSION/run-last.json" 2> "$SESSION/run-last.err"
    say "tick end rc=$? $(tail -c 300 "$SESSION/run-last.json" | tr '\n' ' ')"
    # A killed tick's lease outlives it until it expires: back off.
    grep -q LEASE_HELD "$SESSION/run-last.err" && { sleep 55 & wait $!; }
    [ -n "$wd" ] && { kill "$wd" 2>/dev/null; wait "$wd" 2>/dev/null; }
    sleep 5
  done
}

# --- watch --------------------------------------------------------------------
snapshot() {
  cli campaign status "$1" 2>/dev/null | python3 -c '
import json,sys,subprocess
d=json.load(sys.stdin)
for a in d["agent"]:
  if a.get("phase") in ("needs_agent","ready"):
    b=json.loads(subprocess.run([sys.argv[1],"--output","json","campaign","batch",sys.argv[2],str(a["batch"])],capture_output=True,text=True).stdout)
    print(a["batch"],a["phase"],b["version"])' "$B" "$1"
}
watch() {
  local C=$1 seen=$SESSION/agent-seen.txt st new
  if [ "${2:-}" = --seen ]; then snapshot "$C" > "$seen"; return; fi
  echo $$ > "$SESSION/watch-$$.pid"
  trap 'rm -f "$SESSION/watch-$$.pid"' EXIT
  touch "$seen"
  while true; do
    st=$(status_of "$C" | head -1) || { sleep 60; continue; }
    [ "$st" = running ] || { echo "STOPPED $st"; return; }
    new=$(snapshot "$C" | grep -vxF -f "$seen")
    [ -n "$new" ] && { echo "AGENT $new" | tr '\n' ' '; echo; return; }
    alive driver || { echo "NO_DRIVER"; return; }
    sleep 120
  done
}

# --- commands -------------------------------------------------------------------
cooldown_end() {  # epoch seconds when the last cooling provider is back, or 0
  cli campaign status "$1" | json 'max([int(c["until"]) for c in (d.get("agent_cooldowns") or {}).values()] or [0])'
}
agent_session() {  # pid of the Claude Code process this command runs under, if any
  local p=$$
  while [ "$p" -gt 1 ]; do
    [ "$(cat "/proc/$p/comm" 2>/dev/null)" = claude ] && { echo "$p"; return 0; }
    p=$(awk '/^PPid:/{print $2}' "/proc/$p/status" 2>/dev/null) || return 1
    [ -n "$p" ] || return 1
  done
  return 1
}
resume_when_ready() {  # wait for the wrap-up and the drafting cooldown, then start, only while the agent session lives
  local C=$1 end now agent told=""
  # Only from inside a Claude Code session, and only while it stays open:
  # a closed session never resumes a campaign (session run policy).
  agent=$(agent_session) || { echo "resume-when-ready runs only from a Claude Code session" >&2; return 1; }
  session_open() { kill -0 "$agent" 2>/dev/null; }
  while true; do
    session_open || { say "resume when ready: agent session $agent closed; not resuming"; return 1; }
    alive driver && { sleep 60; continue; }
    # Only a wrap-up: any other stop is for the agent to look at first.
    status_of "$C" | grep -q "$WRAPPED" || { echo "campaign not wrapped up: $(status_of "$C" | tr '\n' ' ')"; return 1; }
    end=$(cooldown_end "$C") now=$(date +%s)
    [ "${end:-0}" -le "$now" ] && break
    [ "$told" = "$end" ] || say "resume when ready: drafting cools down until $(date -d "@$end" +%H:%M)"
    told=$end
    sleep $(( end - now + 60 < 60 ? end - now + 60 : 60 ))
  done
  # The session must also still be checking in: its 15-minute check beats.
  if [ "$HEARTBEAT_SECONDS" != 0 ] && session_gone; then
    say "resume when ready: no heartbeat for over ${HEARTBEAT_SECONDS}s; not resuming"; return 1
  fi
  start "$C"
}
start() {
  local C=$1 s
  if alive driver; then echo "driver already running (pid $(cat "$SESSION/driver.pid"))"; return 0; fi
  rm -f "$SESSION/stop"
  if ! curl -fs -m 3 http://127.0.0.1:8765 > /dev/null; then
    echo "Anki is not open (AnkiConnect on 127.0.0.1:8765): open it, then start again" >&2; return 1
  fi
  if ! curl -fs -m 2 http://127.0.0.1:50021/version > /dev/null && [ -x /usr/lib/VOICEVOX/vv-engine/run ]; then
    systemctl --user reset-failed linguist-voicevox.service 2>/dev/null
    systemd-run --user --unit=linguist-voicevox --collect /usr/lib/VOICEVOX/vv-engine/run >> "$LOG" 2>&1 &&
      touch "$SESSION/voicevox-started" && say "started VOICEVOX"
  fi
  mapfile -t s < <(status_of "$C")
  if [ "${s[0]:-}" = paused ] && grep -q "$WRAPPED" <<<"${s[1]:-}"; then
    say "resuming the wrapped-up campaign"
    cli campaign requeue "$C" --reason AGENT: --reason APPLY_FAILED: --reason PREPARE_FAILED: >> "$LOG" 2>&1
    cli campaign resume "$C" >> "$LOG" 2>&1
  elif [ "${s[0]:-}" = paused ]; then
    # Any other pause (a step failure, a guardrail): the session's operator
    # fixed the cause and starts again, so start resumes it.
    say "resuming the campaign paused for: ${s[1]:-}"
    cli campaign resume "$C" >> "$LOG" 2>&1
  fi
  echo "$C" > "$SESSION/campaign"
  touch "$SESSION/heartbeat"
  sha256sum "$0" | cut -d' ' -f1 > "$SESSION/driver.script"
  setsid "$REPO/scripts/campaign-session.sh" _drive "$C" > /dev/null 2>> "$LOG" < /dev/null &
  echo $! > "$SESSION/driver.pid"
  say "driver started (pid $!) for $C"
}
show() {
  local C=$1
  alive driver && echo "driver: running (pid $(cat "$SESSION/driver.pid"))" || echo "driver: not running"
  alive driver && [ "$(cat "$SESSION/driver.script" 2>/dev/null)" != "$(sha256sum "$0" | cut -d' ' -f1)" ] &&
    echo "driver: OUTDATED (runs an older campaign-session.sh): wrap-up, then start"
  [ -f "$SESSION/heartbeat" ] && echo "heartbeat: $(( $(date +%s) - $(stat -c %Y "$SESSION/heartbeat") ))s ago (wraps up after ${HEARTBEAT_SECONDS}s)"
  ls "$SESSION"/watch-*.pid > /dev/null 2>&1 && echo "watchers: $(cat "$SESSION"/watch-*.pid | tr '\n' ' ')"
  curl -fs -m 2 http://127.0.0.1:50021/version > /dev/null && echo "VOICEVOX: up" || echo "VOICEVOX: down"
  curl -fs -m 2 http://127.0.0.1:8765 > /dev/null && echo "Anki: open" || echo "Anki: closed"
  cli campaign status "$C" | json '"campaign: %s %s\napplied %s, deferred %s\n%s" % (d["status"], d.get("detail") or "", d["applied_notes"], d["deferred_notes"], d["notes_by_phase"])'
}
wrap_up() {
  local C=$1 pid rc
  touch "$SESSION/stop"  # the driver starts no new tick and resumes nothing
  for f in "$SESSION"/watch-*.pid; do [ -f "$f" ] && kill "$(cat "$f")" 2>/dev/null; rm -f "$f"; done
  echo "pausing; waiting for the running tick to finish its step"
  cli campaign wrap-up "$C" > "$SESSION/handoff.json"; rc=$?
  if alive driver; then
    pid=$(cat "$SESSION/driver.pid")
    # The tick has stopped: end the driver's group (its sleeps, a probe).
    [ $rc = 0 ] && kill -- -"$pid" 2>/dev/null
  fi
  if [ $rc = 0 ]; then
    rm -f "$SESSION/driver.pid"
    if [ -f "$SESSION/voicevox-started" ]; then
      systemctl --user stop linguist-voicevox.service 2>/dev/null && echo "stopped VOICEVOX"
      rm -f "$SESSION/voicevox-started"
    fi
    # Free the GPU: unload the local model (the Ollama service stays).
    model=$(setting llm.model | tr -d '"')
    [ -n "$model" ] && [ "$model" != null ] && command -v ollama > /dev/null && ollama stop "$model" 2>/dev/null
  else
    echo "a tick still runs; run wrap-up again to finish" >&2
  fi
  python3 - "$SESSION/handoff.json" <<'EOF'
import json,sys
d=json.load(open(sys.argv[1]))
print(f"campaign {d['campaign']}: {d['status']} ({d.get('detail') or ''})")
print(f"tick {d['tick']}; agents ended {d['agents_ended']}")
print(f"applied {d['applied_notes']}, deferred {d['deferred_notes']}, batches for the agent {d['agent_batches']}")
print(f"notes by phase {d['notes_by_phase']}")
for p,c in d["quota_cooldowns"].items(): print(f"{p} cooling down until {c['until']}")
print("resume: scripts/campaign-session.sh start (or resume-when-ready to wait out a cooldown)")
EOF
  return $rc
}

command=${1:-}; shift || true
seen=""
[ "${1:-}" = --seen ] && { seen=--seen; shift; }
case $command in
  _drive) echo $$ > "$SESSION/driver.pid"; drive "$1"; rm -f "$SESSION/driver.pid" ;;
  start|watch|status|wrap-up|resume-when-ready|heartbeat)
    C=$(campaign_id "${1:-}") || exit 1
    case $command in
      start) start "$C" ;;
      watch) watch "$C" $seen ;;
      status) show "$C" ;;
      wrap-up) wrap_up "$C" ;;
      resume-when-ready) resume_when_ready "$C" ;;
      heartbeat) touch "$SESSION/heartbeat" ;;
    esac ;;
  *) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
