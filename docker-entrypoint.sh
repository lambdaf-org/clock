#!/bin/sh
set -eu

DB_PATH="${CLOCK_DB_PATH:-/data/clock.db}"

# One-time, opt-in user merge. Keyed by user_id (the stable Discord account id),
# because the username column holds the mutable display name. Pick one:
#
#   CLOCK_MERGE_DEDUPE_ALL=1                        auto-fix the leaderboard: fold
#                                                   every display name shared by
#                                                   more than one account into the
#                                                   account with the most hours
#                                                   (the smaller id is overridden)
#   CLOCK_MERGE_DEDUPE_NAME=<name>                  same, for one display name only
#   CLOCK_MERGE_INTO_ID / CLOCK_MERGE_FROM_ID       merge two specific accounts
#   CLOCK_MERGE_INTO_NAME / CLOCK_MERGE_FROM_NAME   same, by name (must be unique)
#   CLOCK_MERGE_NAME                                optional canonical display name
#
# Guarded so the migration can never stop the bot from starting.
if [ -n "${CLOCK_MERGE_DEDUPE_ALL:-}${CLOCK_MERGE_DEDUPE_NAME:-}" ] \
  || { [ -n "${CLOCK_MERGE_INTO_ID:-}${CLOCK_MERGE_INTO_NAME:-}" ] && [ -n "${CLOCK_MERGE_FROM_ID:-}${CLOCK_MERGE_FROM_NAME:-}" ]; }; then
  if [ -f "$DB_PATH" ]; then
    MERGE_ARGS=""
    [ -n "${CLOCK_MERGE_DEDUPE_ALL:-}" ]  && MERGE_ARGS="$MERGE_ARGS --dedupe-all"
    [ -n "${CLOCK_MERGE_DEDUPE_NAME:-}" ] && MERGE_ARGS="$MERGE_ARGS --dedupe-name ${CLOCK_MERGE_DEDUPE_NAME}"
    [ -n "${CLOCK_MERGE_INTO_ID:-}" ]     && MERGE_ARGS="$MERGE_ARGS --into-id ${CLOCK_MERGE_INTO_ID}"
    [ -n "${CLOCK_MERGE_INTO_NAME:-}" ]   && MERGE_ARGS="$MERGE_ARGS --into-name ${CLOCK_MERGE_INTO_NAME}"
    [ -n "${CLOCK_MERGE_FROM_ID:-}" ]     && MERGE_ARGS="$MERGE_ARGS --from-id ${CLOCK_MERGE_FROM_ID}"
    [ -n "${CLOCK_MERGE_FROM_NAME:-}" ]   && MERGE_ARGS="$MERGE_ARGS --from-name ${CLOCK_MERGE_FROM_NAME}"
    [ -n "${CLOCK_MERGE_NAME:-}" ]        && MERGE_ARGS="$MERGE_ARGS --name ${CLOCK_MERGE_NAME}"

    echo "[clock] running one-time user merge on $DB_PATH"
    # shellcheck disable=SC2086
    python3 /usr/local/bin/merge_clock_user.py "$DB_PATH" $MERGE_ARGS || echo "[clock] user merge failed, continuing"
  else
    echo "[clock] skipping user merge: database not found at $DB_PATH"
  fi
fi

# Dump the current DB to the logs on every startup, so the stats are
# recoverable from the logs if the /data volume is ever lost or corrupted.
# Best-effort: a failed dump must never stop the bot from starting.
if [ -f "$DB_PATH" ]; then
  python3 /usr/local/bin/dump_db_stats.py "$DB_PATH" || echo "[clock] db stats dump failed, continuing"
else
  echo "[clock] no database at $DB_PATH yet, skipping stats dump"
fi

exec "$@"
