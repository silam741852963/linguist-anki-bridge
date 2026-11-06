#!/bin/bash
# campaign-session.sh: run a WP-24 campaign from an interactive session and
# end that session cleanly. Nothing here is a timer: the driver runs only
# between `start` and `wrap-up`, and Anki is never launched (it must already
# be open).
#
#   campaign-session.sh start [CAMPAIGN]    start VOICEVOX if needed, resume a
#                                           paused campaign, run the driver;
#                                           from a Claude Code session, also
#                                           the resume waiter for that session
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
#   campaign-session.sh wait-change [CAMPAIGN]
#                                           return when the driver stops or
#                                           starts: the agent session turns
#                                           its 15-minute check off or on
#   campaign-session.sh resume-when-ready [CAMPAIGN]
#                                           after each automatic wrap-up (no
#                                           provider can draft), wait out the
#                                           cooldown and start again, for as
#                                           long as the Claude Code session that
#                                           runs it stays open; never after the
#                                           user's wrap-up; an agent session runs
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
# Careful mode: when the drafting cap comes within FAST_CAP_SECONDS (7200) of
# a start at full speed, the next start tunes the campaign to prepare one
# batch at a time (`campaign tune --prepare-ahead 1 --prepare-parallel 1`).
# Careful mode ends only when its ramp reaches the saved limits: a careful
# run is long because it is careful, and ending it for that sent the next
# start straight to 20 batches ahead, 4 at a time (2026-11-02: 7% to 71% of
# the Codex window in 37 minutes, 6 points past the cap).
# While the driver runs in careful mode, it speeds up again by itself: every
# RAMP_SECONDS (900) between ticks it compares how fast Codex spends its own
# five-hour window with what llm.agents.codex.max_window_percent leaves until
# the window resets, and steps the limits along RAMP_LADDER ("1 1,2 1,4 2,8
# 3", then the saved limits): up as far as the pace has room for, one step
# down when it spends faster. At the saved limits careful mode ends.
# A one-window override of llm.agents.codex.max_window_percent (the user's
# "this time only") is recorded in session/window-override as "VALUE
# RESETS_AT"; start and the driver put VALUE back once that window resets.
# The driver is detached from the agent session that started it, so it also
# wraps up when that session closes (its Claude Code process exits; checked
# every two minutes) or stops checking in (no `heartbeat` for
# HEARTBEAT_SECONDS). `status` says when the driver
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
FAST_CAP_SECONDS=${FAST_CAP_SECONDS:-7200}    # a cap this soon after a full-speed start: careful mode
RAMP_SECONDS=${RAMP_SECONDS:-900}              # careful mode: how often the pace is judged
APPROVAL_SECONDS=${APPROVAL_SECONDS:-1200}      # after a start, ready batches hold the session this long
BUSY_SECONDS=${BUSY_SECONDS:-300}              # a tool command this young counts as a heartbeat
RAMP_LADDER=${RAMP_LADDER:-"1 1,2 1,4 2,8 3"}  # careful mode: prepare-ahead prepare-parallel steps
WINDOW=${XDG_STATE_HOME:-$HOME/.local/state}/linguist-anki-bridge/agents/window-codex.json
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
  # gives, so counting it kept an idle session from ever wrapping up. Only
  # for APPROVAL_SECONDS after a start: a start during a drafting cooldown
  # wrapped up at once, before the agent could approve what was ready (and
  # approvals are refused while the campaign is paused).
  local phases="approved applying verifying merging" started
  started=$(cat "$SESSION/started-at" 2>/dev/null || echo 0)
  [ $(( $(date +%s) - started )) -lt "$APPROVAL_SECONDS" ] && phases="$phases ready"
  [ "$(cli campaign status "$1" | PHASES="$phases" json 'sum(d["phases"].get(p, 0) for p in __import__("os").environ["PHASES"].split())')" != 0 ]
}
agent_alive() {  # the Claude Code process with this pid still runs
  [ "$(cat "/proc/$1/comm" 2>/dev/null)" = claude ]
}
careful_judge() {  # the drafting cap came: soon after a full-speed start means careful mode
  local started ran
  started=$(cat "$SESSION/started-at" 2>/dev/null || echo 0)
  ran=$(( $(date +%s) - started ))
  # In careful mode the ramp alone ends it (at the saved limits).
  if [ ! -f "$SESSION/careful" ] && [ "$ran" -lt "$FAST_CAP_SECONDS" ]; then
    say "drafting cap ${ran}s after the start: careful mode (one batch at a time) from the next start"
    touch "$SESSION/careful"
  fi
}
ramp_decide() {  # careful mode: the ladder step the Codex window's pace has room for
  python3 - "$WINDOW" "$SESSION/ramp" "$(setting llm.agents.codex.max_window_percent)" \
    "$RAMP_LADDER,$(cat "$SESSION/limits-before")" "$(date +%s)" <<'PY'
import json, math, sys
window_path, ramp_path, limit, ladder, now = sys.argv[1:]
now, ladder = int(now), [s.split() for s in ladder.split(",")]
try: limit = float(json.loads(limit))
except Exception: limit = 0
try: ramp = json.load(open(ramp_path))
except Exception: ramp = {}
level = min(int(ramp.get("level", 0)), len(ladder) - 1)
try: w = json.load(open(window_path))
except Exception: w = {}
used, resets = float(w.get("used_percent", 0)), int(w.get("resets_at", 0))
if resets <= now: used = 0.0  # the window reset since Codex last reported it
left = max(resets - now, 60)
# Spent per second since the last judgment in the same window. The first
# look only measures: the window's own average holds use from before.
measured = ramp.get("resets") == resets and now - ramp.get("at", now) >= 300
rate = max(used - ramp["used"], 0) / (now - ramp["at"]) if measured else 0.0
room = (limit - used) / left if limit and resets > now else math.inf
ahead = lambda l: max(int(ladder[l][0]), 1)
if limit and resets > now and used >= limit:
    to, why = 0, f"window at {used:.0f}% of {limit:.0f}%"
elif not measured and resets > now:
    to, why = level, "first look in this window: measuring the pace"
elif rate > room:
    to, why = max(level - 1, 0), "spending faster than the budget"
elif rate == 0:
    to, why = min(level + 1, len(ladder) - 1), "nothing spent since the last look"
else:
    # Throughput grows with prepare-ahead: the highest step whose pace still
    # leaves a quarter of the budget, two steps at most per judgment.
    to = level
    while to + 1 < len(ladder) and to + 1 <= level + 2 and rate * ahead(to + 1) / ahead(level) * 1.25 <= room:
        to += 1
    why = "the budget has room" if to > level else "pace fits the budget"
json.dump({"level": to, "used": used, "resets": resets, "at": now}, open(ramp_path, "w"))
pace = f"{used:.0f}% of {limit:.0f}% used, {rate*3600:.1f}%/h spent, {('%.1f' % (room*3600)) if room != math.inf else 'any'}%/h allowed"
print(to, " ".join(ladder[to]), "top" if to == len(ladder) - 1 else "", f"| {why}; {pace}")
PY
}
careful_ramp() {  # careful mode while the driver runs: speed up as the Codex window allows
  local C=$1 last level ahead parallel top why before
  [ -f "$SESSION/careful" ] && [ -f "$SESSION/limits-before" ] || return 0
  last=$(cat "$SESSION/ramp-at" 2>/dev/null || cat "$SESSION/started-at" 2>/dev/null || echo 0)
  [ $(( $(date +%s) - last )) -ge "$RAMP_SECONDS" ] || return 0
  date +%s > "$SESSION/ramp-at"
  before=$(cli campaign status "$C" | json '"%d %d" % (d["limits"]["prepare_ahead"], d["limits"]["prepare_parallel"])')
  read -r level ahead parallel top why <<<"$(ramp_decide)"
  [ -n "$ahead" ] || return 0
  [ "$top" = top ] || why="$top $why"
  if [ "$top" = top ]; then
    cli campaign tune "$C" --prepare-ahead "$ahead" --prepare-parallel "$parallel" >> "$LOG" 2>&1 &&
      say "careful mode over while running: prepare-ahead $ahead, prepare-parallel $parallel again ${why#| }" &&
      rm -f "$SESSION/careful" "$SESSION/limits-before" "$SESSION/ramp" "$SESSION/ramp-at"
  elif [ "$ahead $parallel" != "$before" ]; then
    cli campaign tune "$C" --prepare-ahead "$ahead" --prepare-parallel "$parallel" >> "$LOG" 2>&1 &&
      say "careful mode: prepare-ahead $ahead, prepare-parallel $parallel (was $before) ${why#| }"
  fi
}
window_restore() {  # a one-window override of llm.agents.codex.max_window_percent ends when that window resets
  local f=$SESSION/window-override value resets config
  [ -f "$f" ] || return 0
  read -r value resets < "$f"
  [ "$(date +%s)" -ge "${resets:-0}" ] || return 0
  config=${XDG_CONFIG_HOME:-$HOME/.config}/linguist-anki-bridge/config.toml
  sed "s/^\"llm.agents.codex.max_window_percent\" = .*/\"llm.agents.codex.max_window_percent\" = $value/" "$config" > "$config.new" &&
    mv "$config.new" "$config" && rm -f "$f" &&
    say "the Codex window reset: llm.agents.codex.max_window_percent $value again"
}
careful_apply() {  # before a start: one batch at a time in careful mode, the saved limits again after it
  local C=$1
  if [ -f "$SESSION/careful" ]; then
    [ -f "$SESSION/limits-before" ] ||
      cli campaign status "$C" | json '"%d %d" % (d["limits"]["prepare_ahead"], d["limits"]["prepare_parallel"])' > "$SESSION/limits-before"
    rm -f "$SESSION/ramp" "$SESSION/ramp-at"
    cli campaign tune "$C" --prepare-ahead 1 --prepare-parallel 1 >> "$LOG" 2>&1 &&
      say "careful mode: prepare one batch at a time (was $(cat "$SESSION/limits-before"))"
  elif [ -f "$SESSION/limits-before" ]; then
    local ahead parallel
    read -r ahead parallel < "$SESSION/limits-before"
    cli campaign tune "$C" --prepare-ahead "$ahead" --prepare-parallel "$parallel" >> "$LOG" 2>&1 &&
      say "careful mode over: prepare-ahead $ahead, prepare-parallel $parallel again" && rm -f "$SESSION/limits-before"
  fi
}
session_gone() {  # the agent session closed, or stopped checking in
  local beat agent
  # Closed: the Claude Code session that started the driver has exited.
  agent=$(cat "$SESSION/driver.agent" 2>/dev/null)
  [ -n "$agent" ] && ! agent_alive "$agent" && return 0
  [ "$HEARTBEAT_SECONDS" = 0 ] && return 1
  # Busy: a long agent turn (fixing a finding) runs no 15-minute check, since
  # the check fires only between turns; a tool command the session started
  # in the last BUSY_SECONDS counts as a heartbeat. Its background watchers
  # run longer and do not.
  if [ -n "$agent" ] && ps -o etimes= --ppid "$agent" 2>/dev/null |
    awk -v busy="$BUSY_SECONDS" '$1 < busy { found = 1 } END { exit !found }'; then
    touch "$SESSION/heartbeat"
  fi
  beat=$(stat -c %Y "$SESSION/heartbeat" 2>/dev/null || echo 0)
  [ $(( $(date +%s) - beat )) -gt "$HEARTBEAT_SECONDS" ]
}
time_to_wrap_up() {  # nothing can draft and nothing is left to write, or no session watches
  session_gone || { ! drafting_available "$1" && ! writes_pending "$1"; }
}
auto_wrap_up() {  # in its own process group: wrap-up ends the driver's group
  local auto=1
  if session_gone; then say "the agent session closed or sent no heartbeat for over ${HEARTBEAT_SECONDS}s; wrapping up"; auto=""
  else
    say "no provider can draft ($(cli campaign status "$1" | json 'd.get("agent_cooldowns")')); wrapping up"
    careful_judge "$1"
  fi
  AUTO_WRAP=$auto setsid "$REPO/scripts/campaign-session.sh" wrap-up "$1" >> "$LOG" 2>&1 < /dev/null &
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
    window_restore
    careful_ramp "$C"
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
resume_when_ready() {  # while the agent session lives: after each automatic wrap-up, wait out the cooldown and start again
  local C=$1 agent=${2:-} end now told=""
  # Only for a Claude Code session, and only while it stays open: a closed
  # session never resumes a campaign (session run policy). `start` passes the
  # session's pid; run by hand, the session is found among the ancestors.
  [ -n "$agent" ] || agent=$(agent_session) || { echo "resume-when-ready runs only from a Claude Code session" >&2; return 1; }
  if [ -f "$SESSION/waiter.pid" ] && kill -0 "$(cat "$SESSION/waiter.pid")" 2>/dev/null; then
    echo "a resume-when-ready waiter already runs (pid $(cat "$SESSION/waiter.pid"))"; return 0
  fi
  echo $$ > "$SESSION/waiter.pid"
  echo "$agent" > "$SESSION/waiter.agent"
  trap '[ "$(cat "$SESSION/waiter.pid" 2>/dev/null)" = $$ ] && rm -f "$SESSION/waiter.pid"' EXIT
  session_open() { agent_alive "$agent"; }
  while true; do
    session_open || { say "resume when ready: agent session $agent closed; not resuming"; return 1; }
    alive driver && { sleep 60; continue; }
    # The user resumed a campaign the driver stopped for (469: a guardrail
    # pause, then `campaign resume` by hand, and no driver ran): start it.
    if status_of "$C" | head -n 1 | grep -qx running; then
      say "resume when ready: the campaign was resumed; starting"
      AGENT_PID=$agent start "$C" || return 1
      told=""; sleep 60; continue
    fi
    # The user's own wrap-up ends the waiter.
    if [ ! -f "$SESSION/auto-wrapped" ] && status_of "$C" | grep -q "$WRAPPED"; then
      say "resume when ready: wrapped up by the user; stopping"; return 1
    fi
    # Any other pause (a guardrail, a lost heartbeat, a step failure) is the
    # user's to lift: wait for their `campaign resume`.
    if [ ! -f "$SESSION/auto-wrapped" ] || ! status_of "$C" | grep -q "$WRAPPED"; then
      [ "$told" = paused ] || say "resume when ready: paused ($(status_of "$C" | tr '\n' ' ')); waiting for the user to resume"
      told=paused; sleep 60; continue
    fi
    end=$(cooldown_end "$C") now=$(date +%s)
    if [ "${end:-0}" -gt "$now" ]; then
      [ "$told" = "$end" ] || say "resume when ready: drafting cools down until $(date -d "@$end" +%H:%M)"
      told=$end
      sleep 60; continue
    fi
    # No heartbeat needed here: the session's 15-minute check is off while
    # the campaign is wrapped up, and the bound process proves the session.
    say "resume when ready: cooldown over; starting"
    AGENT_PID=$agent start "$C" || return 1
    told=""
    sleep 60
  done
}
wait_change() {  # return when the driver stops (WRAPPED) or starts (RUNNING): the agent session turns its 15-minute check off or on
  local was now
  alive driver && was=RUNNING || was=WRAPPED
  while true; do
    sleep 20
    alive driver && now=RUNNING || now=WRAPPED
    [ "$now" != "$was" ] && { echo "$now: $(status_of "$1" | tr '\n' ' ')"; return 0; }
  done
}
start() {
  local C=$1 s
  if alive driver; then echo "driver already running (pid $(cat "$SESSION/driver.pid"))"; return 0; fi
  rm -f "$SESSION/stop" "$SESSION/auto-wrapped"
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
  window_restore
  careful_apply "$C"
  date +%s > "$SESSION/started-at"
  touch "$SESSION/heartbeat"
  # Started from a Claude Code session (any session): that session resumes
  # the campaign after each automatic wrap-up for as long as it stays open.
  local agent
  # The session's Claude Code process: found among the ancestors, or passed
  # by the resume waiter (which runs detached).
  agent=${AGENT_PID:-$(agent_session)}
  echo "$agent" > "$SESSION/driver.agent"  # empty from a plain terminal: heartbeat only
  if [ -n "$agent" ] && [ -z "${AGENT_PID:-}" ] &&
     { [ "$(cat "$SESSION/waiter.agent" 2>/dev/null)" != "$agent" ] ||
       ! { [ -f "$SESSION/waiter.pid" ] && kill -0 "$(cat "$SESSION/waiter.pid")" 2>/dev/null; }; }; then
    # The newest session takes the waiter over from an older one.
    [ -f "$SESSION/waiter.pid" ] && kill "$(cat "$SESSION/waiter.pid")" 2>/dev/null
    rm -f "$SESSION/waiter.pid"
    setsid "$REPO/scripts/campaign-session.sh" _wait "$C" "$agent" > /dev/null 2>> "$LOG" < /dev/null &
    say "resume waiter started for agent session $agent"
  fi
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
  if [ -n "${AUTO_WRAP:-}" ]; then
    touch "$SESSION/auto-wrapped"  # resume-when-ready may resume after the cooldown
  else
    # The user's (or the agent's) wrap-up: nothing resumes it but a new start.
    rm -f "$SESSION/auto-wrapped"
    [ -f "$SESSION/waiter.pid" ] && kill "$(cat "$SESSION/waiter.pid")" 2>/dev/null
    rm -f "$SESSION/waiter.pid"
  fi
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
  _wait) resume_when_ready "$1" "$2" ;;
  start|watch|status|wrap-up|resume-when-ready|heartbeat|wait-change)
    C=$(campaign_id "${1:-}") || exit 1
    case $command in
      start) start "$C" ;;
      watch) watch "$C" $seen ;;
      status) show "$C" ;;
      wrap-up) wrap_up "$C" ;;
      resume-when-ready) resume_when_ready "$C" ;;
      heartbeat) touch "$SESSION/heartbeat" ;;
      wait-change) wait_change "$C" ;;
    esac ;;
  *) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
