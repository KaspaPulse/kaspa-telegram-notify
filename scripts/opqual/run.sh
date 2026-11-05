#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DRY=0
RESUME_ID=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY=1; shift ;;
    --resume) [[ $# -ge 2 ]] || { echo '--resume requires RUN_ID' >&2; exit 2; }; RESUME_ID="$2"; shift 2 ;;
    -h|--help) printf 'Usage: %s [--dry-run] [--resume RUN_ID]\n' "$0"; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ "$DRY" == 0 || -z "$RESUME_ID" ]] || { echo 'dry-run cannot resume a material run' >&2; exit 2; }
export OPQUAL_DRY_RUN="$DRY"
if [[ -n "$RESUME_ID" ]]; then export OPQUAL_RUN_ID="$RESUME_ID" OPQUAL_RESUME_MODE=1; fi
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run

if [[ -n "$RESUME_ID" ]] && state_is cleanup.completed true; then
  die "run $RESUME_ID is already cleaned/closed; start a new run rather than replay material operations"
fi

if [[ "$DRY" == 1 ]]; then
  "$SCRIPT_DIR/preflight.sh" --dry-run
  info "DRY_RUN=PASS no containers, DB operations, application process, or signals created"
  exit 0
fi

record_op RUN PLANNED "one-command operational qualification lifecycle; resume=$RESUME_MODE"
phase_current() {
  local phase="$1"
  case "$phase" in
    provision)
      network_exists "$DOCKER_NETWORK" && volume_exists "$POSTGRES_VOLUME" && container_exists "$POSTGRES_CONTAINER" \
        && [[ "$(sudo -n docker inspect "$POSTGRES_CONTAINER" --format '{{.State.Running}}')" == true ]]
      ;;
    migrate)
      container_exists "$POSTGRES_CONTAINER" && [[ "$(pg_admin -At -c "SELECT to_regclass('public.opqual_harness_migrations') IS NOT NULL;")" == t ]]
      ;;
    fixtures)
      local n; for n in "$TELEGRAM_CONTAINER" "$HTTP_CONTAINER" "$NODE_CONTAINER"; do
        container_exists "$n" || return 1
        [[ "$(sudo -n docker inspect "$n" --format '{{.State.Running}}')" == true ]] || return 1
      done
      ;;
    start_app)
      container_exists "$APP_CONTAINER" && [[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')" == true ]]
      ;;
    create_lock)
      container_exists "$LOCK_CONTAINER" && [[ "$(sudo -n docker inspect "$LOCK_CONTAINER" --format '{{.State.Running}}')" == true ]] \
        && [[ "$(pg_admin -At -c "SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.application_name='$LOCK_APP_NAME' AND l.locktype='advisory' AND l.granted;")" -gt 0 ]]
      ;;
    exec01_shutdown)
      container_exists "$APP_CONTAINER" && [[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')" == false ]] \
        && state_is exec01.clean_exit true && state_is exec01.zero_sessions true && state_is exec01.zero_locks true
      ;;
    restart)
      container_exists "$APP_CONTAINER" && [[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')" == true ]] \
        && state_is exec01.clean_restart true
      ;;
    scenarios) [[ -s "$RUN_DIR/scenario-results.csv" && -s "$RUN_DIR/scenario-summary.json" ]] ;;
    *) return 1 ;;
  esac
}
run_phase() {
  local phase="$1" script="$2"
  if state_is "phase.$phase" VERIFIED; then
    if phase_current "$phase"; then
      record_op "RESUME:$phase" VERIFIED "phase receipt and current side effects agree; skipped replay"
      info "RESUME_SKIP=$phase"
      return 0
    fi
    record_op "RESUME:$phase" UNKNOWN "phase was VERIFIED but current side effects no longer match; refusing blind replay"
    die "resume mismatch for phase $phase; actual state must be reconciled before retry"
  fi
  "$SCRIPT_DIR/$script"
}

FINALIZING=0
finalize() {
  local original_rc="$1"
  [[ "$FINALIZING" == 0 ]] || return 0
  FINALIZING=1
  set +e
  "$SCRIPT_DIR/cleanup.sh"
  local cleanup_rc=$?
  "$SCRIPT_DIR/collect-evidence.sh"
  local evidence_rc=$?
  set -e
  if [[ "$original_rc" -ne 0 ]]; then return "$original_rc"; fi
  if [[ "$cleanup_rc" -ne 0 ]]; then return "$cleanup_rc"; fi
  return "$evidence_rc"
}

on_exit() {
  local rc=$?
  trap - EXIT
  if [[ "$FINALIZING" == 0 ]]; then
    finalize "$rc" || rc=$?
  fi
  exit "$rc"
}
trap on_exit EXIT

"$SCRIPT_DIR/preflight.sh"
run_phase provision provision.sh
run_phase migrate migrate.sh
run_phase fixtures fixtures.sh
run_phase start_app start-app.sh
run_phase create_lock create-lock.sh
run_phase exec01_shutdown exec01.sh
run_phase restart restart.sh
run_phase scenarios run-scenarios.sh

# Success path: cleanup is part of qualification and must complete before final evidence.
"$SCRIPT_DIR/cleanup.sh"
"$SCRIPT_DIR/collect-evidence.sh"
FINALIZING=1
trap - EXIT
record_op RUN VERIFIED "one-command qualification lifecycle finished; see FINAL_RESULT.json"
info "OPQUAL_RUN=COMPLETE run_id=$RUN_ID evidence=$RUN_DIR"