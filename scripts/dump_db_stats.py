#!/usr/bin/env python3
"""Dump ClockBot's database to stdout on startup, so the container logs always
hold a recoverable snapshot of the stats.

ClockBot keeps everything in a single SQLite file on the /data volume. If that
volume is ever lost or corrupted, there is no other copy. This script prints,
on every container start, a human-readable summary plus a full SQL snapshot of
the database to stdout (the container logs). Because logs are usually shipped
off the host, the most recent snapshot survives a volume loss and the database
can be rebuilt from it.

The snapshot is wrapped between marker lines so it can be extracted from the
logs and replayed:

    # pull the latest snapshot out of the logs into a file
    docker logs <container> \\
      | sed -n '/CLOCKBOT-DB-DUMP BEGIN/,/CLOCKBOT-DB-DUMP END/p' \\
      | grep -v CLOCKBOT-DB-DUMP > restore.sql

    # rebuild a fresh database from it
    sqlite3 /data/clock.db < restore.sql

The dump is a complete SQL script (schema + every row of sessions,
weekly_archive, activity_archive, and metadata), so a fresh clock.db restored
from it is identical to the original.

Usage:
    python3 dump_db_stats.py /data/clock.db
"""

from __future__ import annotations

import datetime as _dt
import sqlite3
import sys
from pathlib import Path

TABLES = ("sessions", "weekly_archive", "activity_archive", "metadata")


def table_counts(conn: sqlite3.Connection) -> dict[str, int]:
    counts: dict[str, int] = {}
    for table in TABLES:
        try:
            counts[table] = conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
        except sqlite3.Error:
            pass
    return counts


def print_summary(conn: sqlite3.Connection) -> None:
    """A quick, human-readable view so the stats are eyeballable in the logs."""
    counts = table_counts(conn)
    print("[clock] DB stats summary")
    for table, n in counts.items():
        print(f"[clock]   {table}: {n} rows")

    # Per-user totals from the weekly archive (the durable historical record).
    try:
        rows = conn.execute(
            """
            SELECT username,
                   COUNT(DISTINCT week_label) AS weeks,
                   SUM(total_min)             AS mins
            FROM weekly_archive
            GROUP BY username
            ORDER BY mins DESC
            """
        ).fetchall()
        if rows:
            print("[clock]   per-user (weekly_archive): username | weeks | total")
            for username, weeks, mins in rows:
                mins = mins or 0
                print(f"[clock]     {username} | {weeks} weeks | {mins / 60:.1f}h ({mins} min)")
    except sqlite3.Error:
        pass

    # Per-activity totals across all weeks.
    try:
        rows = conn.execute(
            """
            SELECT activity, SUM(total_min) AS mins
            FROM activity_archive
            GROUP BY activity
            ORDER BY mins DESC
            """
        ).fetchall()
        if rows:
            print("[clock]   per-activity (activity_archive): activity | total")
            for activity, mins in rows:
                mins = mins or 0
                print(f"[clock]     {activity} | {mins / 60:.1f}h ({mins} min)")
    except sqlite3.Error:
        pass


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: dump_db_stats.py <clock.db>", file=sys.stderr)
        return 2
    db = Path(sys.argv[1])
    if not db.exists():
        print(f"[clock] no database at {db} yet, nothing to dump")
        return 0

    ts = _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    # Read-only so the snapshot never locks or mutates the file the bot is about to open.
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        print_summary(conn)
        print(f"===== CLOCKBOT-DB-DUMP BEGIN {ts} =====")
        print("-- Full clock.db SQL snapshot for disaster recovery.")
        print("-- Restore into a fresh database with: sqlite3 clock.db < restore.sql")
        for line in conn.iterdump():
            print(line)
        print("===== CLOCKBOT-DB-DUMP END =====")
    except sqlite3.Error as exc:
        print(f"[clock] db stats dump error: {exc}", file=sys.stderr)
        return 1
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
