#!/usr/bin/env python3
"""
One-time ClockBot DB repair.

Folds data for one Discord username into another, for example:

    x_ -> x

The script updates only rows whose username exactly matches the source username.
Everything else is left as-is.

It updates:
- sessions
- weekly_archive
- activity_archive

Then it merges duplicate archive rows created by the fold:
- weekly_archive: same user_id + username + week_label
- activity_archive: same user_id + username + week_label + activity

Usage:

    python3 scripts/fold_discord_username.py /data/clock.db x

or explicitly:

    python3 scripts/fold_discord_username.py /data/clock.db x --source x_

Dry run:

    python3 scripts/fold_discord_username.py /data/clock.db x --dry-run
"""

from __future__ import annotations

import argparse
import re
import sqlite3
from pathlib import Path


TABLES = ("sessions", "weekly_archive", "activity_archive")


def metadata_key(source: str, target: str) -> str:
    safe_source = re.sub(r"[^A-Za-z0-9_.-]+", "_", source)
    safe_target = re.sub(r"[^A-Za-z0-9_.-]+", "_", target)
    return f"username_fold_{safe_source}_to_{safe_target}"


def ensure_metadata_table(conn: sqlite3.Connection) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS metadata (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )
        """
    )


def already_ran(conn: sqlite3.Connection, key: str) -> bool:
    row = conn.execute("SELECT value FROM metadata WHERE key = ?", (key,)).fetchone()
    return row is not None and row[0] == "true"


def user_ids_for_username(conn: sqlite3.Connection, username: str) -> list[str]:
    ids: set[str] = set()

    for table in TABLES:
        rows = conn.execute(
            f"SELECT DISTINCT user_id FROM {table} WHERE username = ?",
            (username,),
        ).fetchall()
        ids.update(row[0] for row in rows)

    return sorted(ids)


def count_rows(conn: sqlite3.Connection, table: str, username: str) -> int:
    return conn.execute(
        f"SELECT COUNT(*) FROM {table} WHERE username = ?",
        (username,),
    ).fetchone()[0]


def merge_weekly_archive(conn: sqlite3.Connection) -> int:
    duplicate_groups = conn.execute(
        """
        SELECT user_id, username, week_label, SUM(total_min) AS total, MIN(id) AS keep_id, COUNT(*) AS cnt
        FROM weekly_archive
        GROUP BY user_id, username, week_label
        HAVING cnt > 1
        """
    ).fetchall()

    deleted = 0

    for user_id, username, week_label, total, keep_id, _cnt in duplicate_groups:
        conn.execute(
            "UPDATE weekly_archive SET total_min = ? WHERE id = ?",
            (total, keep_id),
        )
        deleted += conn.execute(
            """
            DELETE FROM weekly_archive
            WHERE user_id = ?
              AND username = ?
              AND week_label = ?
              AND id <> ?
            """,
            (user_id, username, week_label, keep_id),
        ).rowcount

    return deleted


def merge_activity_archive(conn: sqlite3.Connection) -> int:
    duplicate_groups = conn.execute(
        """
        SELECT user_id, username, week_label, activity, SUM(total_min) AS total, MIN(id) AS keep_id, COUNT(*) AS cnt
        FROM activity_archive
        GROUP BY user_id, username, week_label, activity
        HAVING cnt > 1
        """
    ).fetchall()

    deleted = 0

    for user_id, username, week_label, activity, total, keep_id, _cnt in duplicate_groups:
        conn.execute(
            "UPDATE activity_archive SET total_min = ? WHERE id = ?",
            (total, keep_id),
        )
        deleted += conn.execute(
            """
            DELETE FROM activity_archive
            WHERE user_id = ?
              AND username = ?
              AND week_label = ?
              AND activity = ?
              AND id <> ?
            """,
            (user_id, username, week_label, activity, keep_id),
        ).rowcount

    return deleted


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Fold ClockBot rows from one Discord username into another."
    )
    parser.add_argument("db", type=Path, help="Path to clock.db")
    parser.add_argument("target", help="Canonical username to keep, e.g. x")
    parser.add_argument(
        "--source",
        help="Username to fold into target. Defaults to '<target>_'.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print what would change, then roll back.",
    )

    args = parser.parse_args()

    source = args.source or f"{args.target}_"
    run_key = metadata_key(source, args.target)

    if source == args.target:
        raise SystemExit("source and target must be different")

    if not args.db.exists():
        raise SystemExit(f"database not found: {args.db}")

    conn = sqlite3.connect(args.db)
    conn.execute("PRAGMA foreign_keys = ON")

    try:
        ensure_metadata_table(conn)

        if already_ran(conn, run_key):
            print(f"username fold already ran: {source} -> {args.target}")
            return 0

        target_ids = user_ids_for_username(conn, args.target)
        source_ids = user_ids_for_username(conn, source)

        print(f"target username: {args.target}")
        print(f"source username: {source}")
        print(f"target user_ids: {target_ids or '(none)'}")
        print(f"source user_ids: {source_ids or '(none)'}")

        if not source_ids:
            print("nothing to do: no rows found for source username")
            return 0

        # If target exists, fold source rows into the existing canonical user_id.
        # If target does not exist yet, keep the source user_id and just rename it.
        if len(target_ids) > 1:
            raise SystemExit(
                f"target username '{args.target}' has multiple user_ids: {target_ids}; fix manually first"
            )

        if len(source_ids) > 1 and not target_ids:
            raise SystemExit(
                f"source username '{source}' has multiple user_ids and no target user_id exists: {source_ids}; fix manually first"
            )

        target_user_id = target_ids[0] if target_ids else source_ids[0]

        before = {
            table: {
                "source": count_rows(conn, table, source),
                "target": count_rows(conn, table, args.target),
            }
            for table in TABLES
        }

        print("\nbefore:")
        for table, counts in before.items():
            print(f"  {table}: source={counts['source']} target={counts['target']}")

        conn.execute("BEGIN")

        session_updates = conn.execute(
            """
            UPDATE sessions
            SET user_id = ?, username = ?
            WHERE username = ?
            """,
            (target_user_id, args.target, source),
        ).rowcount

        weekly_updates = conn.execute(
            """
            UPDATE weekly_archive
            SET user_id = ?, username = ?
            WHERE username = ?
            """,
            (target_user_id, args.target, source),
        ).rowcount

        activity_updates = conn.execute(
            """
            UPDATE activity_archive
            SET user_id = ?, username = ?
            WHERE username = ?
            """,
            (target_user_id, args.target, source),
        ).rowcount

        weekly_deleted = merge_weekly_archive(conn)
        activity_deleted = merge_activity_archive(conn)

        after = {
            table: {
                "source": count_rows(conn, table, source),
                "target": count_rows(conn, table, args.target),
            }
            for table in TABLES
        }

        print("\nupdated:")
        print(f"  sessions rows moved: {session_updates}")
        print(f"  weekly_archive rows moved: {weekly_updates}")
        print(f"  activity_archive rows moved: {activity_updates}")
        print(f"  weekly_archive duplicate rows merged away: {weekly_deleted}")
        print(f"  activity_archive duplicate rows merged away: {activity_deleted}")

        print("\nafter:")
        for table, counts in after.items():
            print(f"  {table}: source={counts['source']} target={counts['target']}")

        if args.dry_run:
            conn.rollback()
            print("\ndry run: rolled back")
        else:
            conn.execute(
                "INSERT OR REPLACE INTO metadata (key, value) VALUES (?, 'true')",
                (run_key,),
            )
            conn.commit()
            print("\ncommitted")

        return 0

    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()


if __name__ == "__main__":
    raise SystemExit(main())
