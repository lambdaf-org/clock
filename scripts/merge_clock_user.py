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
    # auto-fix the whole leaderboard: fold every duplicate-display-name account
    # into its largest one (overrides the smaller account's user_id)
    merge_clock_user.py /data/clock.db --dedupe-all

    # see every account and any display name shared by more than one account
    merge_clock_user.py /data/clock.db --list

    # two leaderboard rows show the SAME name: merge them in one shot
    # (folds every account with this name into the one with the most hours)
    merge_clock_user.py /data/clock.db --dedupe-name Thirsty

    # merge two specific accounts by Discord user_id (unambiguous)
    merge_clock_user.py /data/clock.db --into-id 111 --from-id 222 --name x

    # by display name (each must resolve to a single user_id, else it skips)
    merge_clock_user.py /data/clock.db --into-name x --from-name x_

    # preview only
    merge_clock_user.py /data/clock.db --dedupe-name Thirsty --dry-run
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


def total_minutes(conn: sqlite3.Connection, uid: str) -> int:
    s = conn.execute(
        "SELECT COALESCE(SUM(minutes),0) FROM sessions WHERE user_id=? AND ended_at IS NOT NULL",
        (uid,),
    ).fetchone()[0]
    w = conn.execute(
        "SELECT COALESCE(SUM(total_min),0) FROM weekly_archive WHERE user_id=?", (uid,)
    ).fetchone()[0]
    return int(s) + int(w)


def perform_merge(conn, target_uid, source_uids, canonical, dry_run) -> None:
    ids = list(dict.fromkeys([target_uid, *source_uids]))
    placeholders = ",".join("?" * len(ids))
    conn.execute("BEGIN")
    for table in TABLES:
        n = conn.execute(
            f"UPDATE {table} SET user_id=?, username=? WHERE user_id IN ({placeholders})",
            (target_uid, canonical, *ids),
        ).rowcount
        log(f"{table}: {n} rows now under {target_uid} as {canonical!r}")
    wm = merge_dupes(conn, "weekly_archive", ("user_id", "week_label"))
    am = merge_dupes(conn, "activity_archive", ("user_id", "week_label", "activity"))
    log(f"weekly_archive duplicates merged: {wm}; activity_archive duplicates merged: {am}")
    if dry_run:
        conn.rollback()
        log("dry run: rolled back, no changes written")
    else:
        conn.commit()
        log("committed")


def do_list(conn: sqlite3.Connection) -> None:
    """Print every account (user_id) with its hours and the display names seen,
    and call out any display name shared by more than one account."""
    rows = conn.execute(
        """
        SELECT user_id, GROUP_CONCAT(DISTINCT username) FROM (
            SELECT user_id, username FROM sessions
            UNION SELECT user_id, username FROM weekly_archive
            UNION SELECT user_id, username FROM activity_archive
        ) GROUP BY user_id
        """
    ).fetchall()
    data = [(uid, total_minutes(conn, uid), names or "") for uid, names in rows]
    log("accounts (user_id | total | display names seen):")
    for uid, mins, names in sorted(data, key=lambda r: -r[1]):
        log(f"  {uid} | {mins / 60:.1f}h | {names}")
    from collections import defaultdict

    by_name: dict[str, set[str]] = defaultdict(set)
    for uid, _, names in data:
        for nm in names.split(","):
            if nm:
                by_name[nm].add(uid)
    dups = {nm: us for nm, us in by_name.items() if len(us) > 1}
    if dups:
        log("duplicate display names (one name, several accounts):")
        for nm, us in dups.items():
            log(f"  {nm!r}: user_ids {sorted(us)}  ->  merge with --dedupe-name {nm}")
    else:
        log("no duplicate display names found")


def do_dedupe_name(conn: sqlite3.Connection, name: str, dry_run: bool) -> None:
    """Merge every account sharing this display name into the one with the most
    hours. The right tool when two leaderboard rows show the same name (so they
    cannot be told apart by name) but are one person."""
    uids = uids_for_name(conn, name)
    if len(uids) < 2:
        log(f"dedupe {name!r}: maps to {len(uids)} account(s), nothing to merge")
        return
    totals = {u: total_minutes(conn, u) for u in uids}
    target = max(uids, key=lambda u: totals[u])
    sources = [u for u in uids if u != target]
    log(f"dedupe {name!r}: " + ", ".join(f"{u}={totals[u] / 60:.1f}h" for u in uids))
    log(f"dedupe {name!r}: keeping {target}, folding {sources} into it")
    perform_merge(conn, target, sources, name, dry_run)


def do_dedupe_all(conn: sqlite3.Connection, dry_run: bool) -> None:
    """Auto-dedupe the all-time leaderboard. For every display name shared by
    more than one user_id, fold the smaller accounts (by all-time hours) into
    the one with the most hours, overriding the smaller accounts' user_id."""
    from collections import defaultdict

    rows = conn.execute(
        """
        SELECT user_id, GROUP_CONCAT(DISTINCT username) FROM (
            SELECT user_id, username FROM sessions
            UNION SELECT user_id, username FROM weekly_archive
            UNION SELECT user_id, username FROM activity_archive
        ) GROUP BY user_id
        """
    ).fetchall()
    by_name: dict[str, set[str]] = defaultdict(set)
    for uid, names in rows:
        for nm in (names or "").split(","):
            if nm:
                by_name[nm].add(uid)
    dups = {nm: us for nm, us in by_name.items() if len(us) > 1}
    if not dups:
        log("dedupe-all: no display name maps to more than one account, nothing to do")
        return

    # Plan from the clean read state, then apply in one transaction.
    plans = []
    for nm, uids in dups.items():
        totals = {u: total_minutes(conn, u) for u in uids}
        keeper = max(uids, key=lambda u: totals[u])
        losers = [u for u in uids if u != keeper]
        plans.append((nm, keeper, losers, totals))

    conn.execute("BEGIN")
    folded = 0
    for nm, keeper, losers, totals in plans:
        ranked = ", ".join(f"{u}={totals[u] / 60:.1f}h" for u in sorted(totals, key=lambda u: -totals[u]))
        log(f"dedupe-all {nm!r}: {ranked}")
        log(f"dedupe-all {nm!r}: keeping {keeper}, overriding {losers} -> {keeper}")
        ids = list(dict.fromkeys([keeper, *losers]))
        placeholders = ",".join("?" * len(ids))
        for table in TABLES:
            conn.execute(
                f"UPDATE {table} SET user_id=?, username=? WHERE user_id IN ({placeholders})",
                (keeper, nm, *ids),
            )
        folded += len(losers)
    wm = merge_dupes(conn, "weekly_archive", ("user_id", "week_label"))
    am = merge_dupes(conn, "activity_archive", ("user_id", "week_label", "activity"))
    log(f"dedupe-all: folded {folded} duplicate account(s); merged {wm} weekly + {am} activity rows")
    if dry_run:
        conn.rollback()
        log("dry run: rolled back, no changes written")
    else:
        conn.commit()
        log("committed")


def main() -> int:
    p = argparse.ArgumentParser(description="Merge one ClockBot user's rows into another, by user_id.")
    p.add_argument("db", type=Path)
    p.add_argument("--into-id")
    p.add_argument("--into-name")
    p.add_argument("--from-id")
    p.add_argument("--from-name")
    p.add_argument("--name", help="Canonical display name to set on the merged account.")
    p.add_argument("--dedupe-name", help="Merge every account sharing this display name into the one with the most hours.")
    p.add_argument("--dedupe-all", action="store_true", help="Auto-fold every duplicate-display-name account in the leaderboard into its largest account.")
    p.add_argument("--list", action="store_true", help="List accounts and any duplicate display names, then exit.")
    p.add_argument("--dry-run", action="store_true")
    args = p.parse_args()

    has_pair = (args.into_id or args.into_name) and (args.from_id or args.from_name)
    if not args.list and not args.dedupe_name and not args.dedupe_all and not has_pair:
        log("nothing to do: pass --list, --dedupe-all, --dedupe-name NAME, or both --into-* and --from-*")
        return 0
    if not args.db.exists():
        log(f"database not found at {args.db}, skipping")
        return 0

    conn = sqlite3.connect(args.db)
    try:
        conn.execute("PRAGMA foreign_keys = ON")
        ensure_metadata(conn)

        if args.list:
            do_list(conn)
            return 0
        if args.dedupe_all:
            do_dedupe_all(conn, args.dry_run)
            return 0
        if args.dedupe_name:
            do_dedupe_name(conn, args.dedupe_name, args.dry_run)
            return 0

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
