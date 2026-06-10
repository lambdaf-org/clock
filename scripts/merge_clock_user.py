#!/usr/bin/env python3
"""One-time ClockBot user merge, keyed by Discord user_id.

Why user_id and not username
-----------------------------
ClockBot stores `msg.author.display_name()` in the `username` column, and that
display name changes over time. Everything that matters (the weekly and all-time
leaderboards, the weekly archive) groups by `user_id`, which is the stable
Discord account id. So the same person can show up under several display-name
labels ("x", "x_") while really being one or two `user_id` values.

Merging by the display-name string is therefore unsafe: a display name is not
unique and is not stable. This script merges by `user_id` instead. You give it
the source and target accounts (by id, or by a display name that resolves to
exactly one id) and it folds the source account's rows into the target, sets one
canonical name, and merges the duplicate archive rows that result.

Safety
------
- It NEVER exits non-zero. If it cannot act safely (ambiguous name, nothing to
  do, or any error) it logs and exits 0, so it can never stop the bot from
  starting. The Docker entrypoint also guards it.
- Idempotent: it records a metadata key and is a no-op on re-run.
- Transactional, with a --dry-run that rolls back.

It touches: sessions, weekly_archive, activity_archive.

Usage:
    # by Discord user_id (recommended, unambiguous)
    merge_clock_user.py /data/clock.db --into-id 111 --from-id 222 --name x

    # by display name (resolved to a single user_id each, else it safely skips)
    merge_clock_user.py /data/clock.db --into-name x --from-name x_

    # preview only
    merge_clock_user.py /data/clock.db --into-id 111 --from-id 222 --dry-run
"""

from __future__ import annotations

import argparse
import re
import sqlite3
import sys
from pathlib import Path

TABLES = ("sessions", "weekly_archive", "activity_archive")


def log(msg: str) -> None:
    print(f"[clock][merge] {msg}", flush=True)


def metadata_key(source_uid: str, target_uid: str) -> str:
    safe = lambda s: re.sub(r"[^A-Za-z0-9_.-]+", "_", s)
    return f"user_merge_{safe(source_uid)}_into_{safe(target_uid)}"


def ensure_metadata(conn: sqlite3.Connection) -> None:
    conn.execute(
        "CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)"
    )


def already_ran(conn: sqlite3.Connection, key: str) -> bool:
    row = conn.execute("SELECT value FROM metadata WHERE key = ?", (key,)).fetchone()
    return row is not None and row[0] == "true"


def uids_for_name(conn: sqlite3.Connection, name: str) -> list[str]:
    uids: set[str] = set()
    for table in TABLES:
        for (uid,) in conn.execute(
            f"SELECT DISTINCT user_id FROM {table} WHERE username = ?", (name,)
        ):
            uids.add(uid)
    return sorted(uids)


def rows_for_uid(conn: sqlite3.Connection, uid: str) -> int:
    return sum(
        conn.execute(f"SELECT COUNT(*) FROM {t} WHERE user_id = ?", (uid,)).fetchone()[0]
        for t in TABLES
    )


def latest_username(conn: sqlite3.Connection, uid: str) -> str | None:
    # The most recent label we have for this account: prefer the newest session.
    row = conn.execute(
        "SELECT username FROM sessions WHERE user_id = ? ORDER BY id DESC LIMIT 1", (uid,)
    ).fetchone()
    if row:
        return row[0]
    row = conn.execute(
        "SELECT username FROM weekly_archive WHERE user_id = ? ORDER BY id DESC LIMIT 1",
        (uid,),
    ).fetchone()
    return row[0] if row else None


def resolve(conn: sqlite3.Connection, role: str, by_id: str | None, by_name: str | None):
    """Resolve one side of the merge to a user_id.

    Returns (user_id_or_None, status) where status is 'ok', 'none' (no such
    rows), or 'ambiguous' (the display name maps to more than one user_id).
    """
    if by_id is not None:
        return by_id, "ok"
    if by_name is None:
        return None, "none"
    uids = uids_for_name(conn, by_name)
    if len(uids) == 1:
        return uids[0], "ok"
    if len(uids) == 0:
        log(f"{role}: no rows found for display name {by_name!r}")
        return None, "none"
    counts = {u: rows_for_uid(conn, u) for u in uids}
    log(
        f"{role}: display name {by_name!r} is ambiguous, it maps to several user_ids: "
        + ", ".join(f"{u} ({counts[u]} rows)" for u in uids)
    )
    log(f"{role}: refusing to guess. Re-run with --{role}-id <user_id> to pick one.")
    return None, "ambiguous"


def merge_dupes(conn: sqlite3.Connection, table: str, key_cols: tuple[str, ...]) -> int:
    cols = ", ".join(key_cols)
    groups = conn.execute(
        f"""
        SELECT {cols}, SUM(total_min) AS total, MIN(id) AS keep_id, COUNT(*) AS cnt
        FROM {table} GROUP BY {cols} HAVING cnt > 1
        """
    ).fetchall()
    deleted = 0
    for row in groups:
        key_vals = row[: len(key_cols)]
        total, keep_id = row[len(key_cols)], row[len(key_cols) + 1]
        conn.execute(f"UPDATE {table} SET total_min = ? WHERE id = ?", (total, keep_id))
        where = " AND ".join(f"{c} = ?" for c in key_cols)
        deleted += conn.execute(
            f"DELETE FROM {table} WHERE {where} AND id <> ?", (*key_vals, keep_id)
        ).rowcount
    return deleted


def main() -> int:
    p = argparse.ArgumentParser(description="Merge one ClockBot user's rows into another, by user_id.")
    p.add_argument("db", type=Path)
    p.add_argument("--into-id")
    p.add_argument("--into-name")
    p.add_argument("--from-id")
    p.add_argument("--from-name")
    p.add_argument("--name", help="Canonical display name to set on the merged account.")
    p.add_argument("--dry-run", action="store_true")
    args = p.parse_args()

    if not (args.into_id or args.into_name) or not (args.from_id or args.from_name):
        log("nothing to do: need a target (--into-id/--into-name) and a source (--from-id/--from-name)")
        return 0
    if not args.db.exists():
        log(f"database not found at {args.db}, skipping")
        return 0

    conn = sqlite3.connect(args.db)
    try:
        conn.execute("PRAGMA foreign_keys = ON")
        ensure_metadata(conn)

        target_uid, t_status = resolve(conn, "into", args.into_id, args.into_name)
        source_uid, s_status = resolve(conn, "from", args.from_id, args.from_name)

        # Any ambiguity means we cannot safely pick an account, so do nothing.
        if t_status == "ambiguous" or s_status == "ambiguous":
            log("not acting: an ambiguous display name needs an explicit --into-id/--from-id")
            return 0
        if source_uid is None:
            log("nothing to merge (source did not resolve to any rows)")
            return 0
        # A name target with no rows yet means a plain rename of the source.
        if target_uid is None:
            if args.into_name:
                target_uid = source_uid
            else:
                log("target did not resolve safely; not acting")
                return 0

        key = metadata_key(source_uid, target_uid)
        if already_ran(conn, key):
            log(f"already merged: {source_uid} into {target_uid}")
            return 0

        canonical = args.name or args.into_name or latest_username(conn, target_uid) \
            or latest_username(conn, source_uid)
        if canonical is None:
            log("could not determine a canonical name; not acting")
            return 0

        log(f"target user_id: {target_uid}  ({rows_for_uid(conn, target_uid)} rows)")
        log(f"source user_id: {source_uid}  ({rows_for_uid(conn, source_uid)} rows)")
        log(f"canonical name: {canonical!r}")
        if source_uid == target_uid:
            log("source and target are the same account; normalising the name and de-duplicating only")

        conn.execute("BEGIN")
        moved = {}
        for table in TABLES:
            moved[table] = conn.execute(
                f"UPDATE {table} SET user_id = ?, username = ? WHERE user_id IN (?, ?)",
                (target_uid, canonical, source_uid, target_uid),
            ).rowcount
        weekly_merged = merge_dupes(conn, "weekly_archive", ("user_id", "week_label"))
        activity_merged = merge_dupes(conn, "activity_archive", ("user_id", "week_label", "activity"))

        for table in TABLES:
            log(f"{table}: {moved[table]} rows now under {target_uid} as {canonical!r}")
        log(f"weekly_archive duplicate rows merged: {weekly_merged}")
        log(f"activity_archive duplicate rows merged: {activity_merged}")

        if args.dry_run:
            conn.rollback()
            log("dry run: rolled back, no changes written")
        else:
            conn.execute(
                "INSERT OR REPLACE INTO metadata (key, value) VALUES (?, 'true')", (key,)
            )
            conn.commit()
            log("committed")
        return 0
    except Exception as exc:  # never take the bot down over a migration
        try:
            conn.rollback()
        except sqlite3.Error:
            pass
        log(f"error, rolled back, leaving data unchanged: {exc}")
        return 0
    finally:
        conn.close()


if __name__ == "__main__":
    sys.exit(main())
