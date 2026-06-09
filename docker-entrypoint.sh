#!/bin/sh
set -eu

DB_PATH="${CLOCK_DB_PATH:-/data/clock.db}"

if [ -n "${CLOCK_FOLD_USERNAME_TARGET:-}" ]; then
  if [ -f "$DB_PATH" ]; then
    SOURCE_ARG=""
    if [ -n "${CLOCK_FOLD_USERNAME_SOURCE:-}" ]; then
      SOURCE_ARG="--source ${CLOCK_FOLD_USERNAME_SOURCE}"
    fi

    echo "[clock] running one-time username fold on $DB_PATH"
    # shellcheck disable=SC2086
    python3 /usr/local/bin/fold_discord_username.py "$DB_PATH" "$CLOCK_FOLD_USERNAME_TARGET" $SOURCE_ARG
  else
    echo "[clock] skipping username fold: database not found at $DB_PATH"
  fi
fi

exec "$@"
