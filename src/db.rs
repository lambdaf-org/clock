use chrono::{Datelike, Duration, NaiveDateTime, Utc};
use chrono_tz::Europe::Zurich;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug)]
pub struct ActiveSession {
    pub id: i64,
    pub user_id: String,
    pub username: String,
    pub activity: String,
    pub started_at: NaiveDateTime,
}

#[derive(Debug)]
pub struct LeaderboardEntry {
    pub username: String,
    pub total_minutes: i64,
}

#[derive(Debug)]
pub struct ActivityEntry {
    pub username: String,
    pub activity: String,
    pub total_minutes: i64,
    pub session_count: i64,
}

#[derive(Debug)]
pub struct WeeklySummary {
    pub total_minutes: i64,
    pub total_sessions: i64,
    pub unique_workers: i64,
    pub mvp: Option<(String, i64)>,
    pub top_activity: Option<(String, i64)>,
    pub longest_session: Option<(String, String, i64)>,
    pub breakdown: Vec<ActivityEntry>,
}

pub fn now_ch() -> NaiveDateTime {
    Utc::now().with_timezone(&Zurich).naive_local()
}

fn now_ch_str() -> String {
    now_ch().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn swiss_week_label() -> String {
    let now = Utc::now().with_timezone(&Zurich);
    now.format("KW%V/%G").to_string()
}

fn monday_of_current_week() -> String {
    let now = now_ch();
    let wd = now.weekday().num_days_from_monday() as i64;
    let monday = now.date() - Duration::days(wd);
    monday.format("%Y-%m-%d 00:00:00").to_string()
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id     TEXT    NOT NULL,
                username    TEXT    NOT NULL,
                activity    TEXT    NOT NULL,
                started_at  TEXT    NOT NULL,
                ended_at    TEXT,
                minutes     INTEGER
            );
            CREATE TABLE IF NOT EXISTS weekly_archive (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id     TEXT    NOT NULL,
                username    TEXT    NOT NULL,
                week_label  TEXT    NOT NULL,
                total_min   INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS activity_archive (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id     TEXT    NOT NULL,
                username    TEXT    NOT NULL,
                week_label  TEXT    NOT NULL,
                activity    TEXT    NOT NULL,
                total_min   INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS metadata (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sess_user   ON sessions(user_id);
            CREATE INDEX IF NOT EXISTS idx_sess_end    ON sessions(ended_at);
            CREATE INDEX IF NOT EXISTS idx_arch_user   ON weekly_archive(user_id);
            CREATE INDEX IF NOT EXISTS idx_actarch_user ON activity_archive(user_id);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn clock_in(&self, user_id: &str, username: &str, activity: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let active: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sessions WHERE user_id=?1 AND ended_at IS NULL",
            params![user_id],
            |r| r.get(0),
        )?;
        if active {
            anyhow::bail!("already clocked in");
        }
        conn.execute(
            "INSERT INTO sessions (user_id,username,activity,started_at) VALUES (?1,?2,?3,?4)",
            params![user_id, username, activity, now_ch_str()],
        )?;
        Ok(())
    }

    pub fn clock_out(&self, user_id: &str) -> anyhow::Result<(i64, String)> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(i64, String, String)> = conn
            .query_row(
                "SELECT id,started_at,activity FROM sessions WHERE user_id=?1 AND ended_at IS NULL",
                params![user_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        match row {
            Some((id, started_str, activity)) => {
                let started = NaiveDateTime::parse_from_str(&started_str, "%Y-%m-%d %H:%M:%S")?;
                let now = now_ch();
                let minutes = (now - started).num_minutes();
                conn.execute(
                    "UPDATE sessions SET ended_at=?1, minutes=?2 WHERE id=?3",
                    params![now_ch_str(), minutes, id],
                )?;
                Ok((minutes, activity))
            }
            None => anyhow::bail!("not clocked in"),
        }
    }

    pub fn active_session(&self, user_id: &str) -> anyhow::Result<Option<ActiveSession>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT id,user_id,username,activity,started_at FROM sessions WHERE user_id=?1 AND ended_at IS NULL",
            params![user_id],
            |r| Ok(ActiveSession {
                id: r.get(0)?,
                user_id: r.get(1)?,
                username: r.get(2)?,
                activity: r.get(3)?,
                started_at: NaiveDateTime::parse_from_str(&r.get::<_,String>(4)?, "%Y-%m-%d %H:%M:%S").unwrap(),
            }),
        ) {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn leaderboard_weekly(&self) -> anyhow::Result<Vec<LeaderboardEntry>> {
        let conn = self.conn.lock().unwrap();
        let monday = monday_of_current_week();
        let mut stmt = conn.prepare(
            "SELECT username, SUM(minutes) as total FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= ?1
             GROUP BY user_id ORDER BY total DESC LIMIT 15",
        )?;
        let rows = stmt.query_map(params![monday], |r| {
            Ok(LeaderboardEntry {
                username: r.get(0)?,
                total_minutes: r.get(1)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn leaderboard_alltime(&self) -> anyhow::Result<Vec<LeaderboardEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT username, SUM(mins) as total FROM (
                SELECT user_id, username, SUM(minutes) as mins FROM sessions
                    WHERE ended_at IS NOT NULL GROUP BY user_id
                UNION ALL
                SELECT user_id, username, SUM(total_min) as mins FROM weekly_archive
                    GROUP BY user_id
             ) GROUP BY user_id ORDER BY total DESC LIMIT 15",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(LeaderboardEntry {
                username: r.get(0)?,
                total_minutes: r.get(1)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn archive_week(&self, week_label: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        // Archive totals per user
        conn.execute(
            "INSERT INTO weekly_archive (user_id,username,week_label,total_min)
             SELECT user_id,username,?1,SUM(minutes) FROM sessions
             WHERE ended_at IS NOT NULL GROUP BY user_id",
            params![week_label],
        )?;
        // Archive per-activity breakdown
        conn.execute(
            "INSERT INTO activity_archive (user_id,username,week_label,activity,total_min)
             SELECT user_id,username,?1,activity,SUM(minutes) FROM sessions
             WHERE ended_at IS NOT NULL GROUP BY user_id, activity",
            params![week_label],
        )?;
        conn.execute("DELETE FROM sessions WHERE ended_at IS NOT NULL", [])?;
        Ok(())
    }

    /// Activity breakdown for current week per user.
    pub fn activity_breakdown_weekly(&self) -> anyhow::Result<Vec<ActivityEntry>> {
        let conn = self.conn.lock().unwrap();
        let monday = monday_of_current_week();
        let mut stmt = conn.prepare(
            "SELECT username, activity, SUM(minutes) as total, COUNT(*) as sessions
             FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= ?1
             GROUP BY user_id, activity
             ORDER BY username ASC, total DESC",
        )?;
        let rows = stmt.query_map(params![monday], |r| {
            Ok(ActivityEntry {
                username: r.get(0)?,
                activity: r.get(1)?,
                total_minutes: r.get(2)?,
                session_count: r.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Activity breakdown for all time (archived + current).
    pub fn activity_breakdown_alltime(&self) -> anyhow::Result<Vec<ActivityEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT username, activity, SUM(mins) as total, SUM(cnt) as sessions FROM (
                SELECT username, activity, SUM(minutes) as mins, COUNT(*) as cnt
                    FROM sessions WHERE ended_at IS NOT NULL
                    GROUP BY user_id, activity
                UNION ALL
                SELECT username, activity, SUM(total_min) as mins, 0 as cnt
                    FROM activity_archive
                    GROUP BY user_id, activity
             ) GROUP BY username, activity ORDER BY username ASC, total DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ActivityEntry {
                username: r.get(0)?,
                activity: r.get(1)?,
                total_minutes: r.get(2)?,
                session_count: r.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Weekly summary data for the automated post.
    pub fn weekly_summary(&self) -> anyhow::Result<WeeklySummary> {
        let conn = self.conn.lock().unwrap();
        let monday = monday_of_current_week();

        // Total hours, total sessions, unique workers
        let (total_min, total_sessions, unique_workers): (i64, i64, i64) = conn.query_row(
            "SELECT COALESCE(SUM(minutes),0), COUNT(*), COUNT(DISTINCT user_id)
             FROM sessions WHERE ended_at IS NOT NULL AND started_at >= ?1",
            params![monday],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        // MVP (most minutes)
        let mvp: Option<(String, i64)> = conn
            .query_row(
                "SELECT username, SUM(minutes) as total FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= ?1
             GROUP BY user_id ORDER BY total DESC LIMIT 1",
                params![monday],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        // Most popular activity
        let top_activity: Option<(String, i64)> = conn
            .query_row(
                "SELECT activity, SUM(minutes) as total FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= ?1
             GROUP BY activity ORDER BY total DESC LIMIT 1",
                params![monday],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        // Longest single session
        let longest_session: Option<(String, String, i64)> = conn
            .query_row(
                "SELECT username, activity, minutes FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= ?1
             ORDER BY minutes DESC LIMIT 1",
                params![monday],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();

        // Per-person breakdown
        let mut stmt = conn.prepare(
            "SELECT username, activity, SUM(minutes) as total
             FROM sessions WHERE ended_at IS NOT NULL AND started_at >= ?1
             GROUP BY user_id, activity ORDER BY username ASC, total DESC",
        )?;
        let rows = stmt.query_map(params![monday], |r| {
            Ok(ActivityEntry {
                username: r.get(0)?,
                activity: r.get(1)?,
                total_minutes: r.get(2)?,
                session_count: 0,
            })
        })?;
        let breakdown: Vec<ActivityEntry> = rows.filter_map(|r| r.ok()).collect();

        Ok(WeeklySummary {
            total_minutes: total_min,
            total_sessions,
            unique_workers,
            mvp,
            top_activity,
            longest_session,
            breakdown,
        })
    }

    pub fn who_is_working(&self) -> anyhow::Result<Vec<ActiveSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,user_id,username,activity,started_at FROM sessions WHERE ended_at IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ActiveSession {
                id: r.get(0)?,
                user_id: r.get(1)?,
                username: r.get(2)?,
                activity: r.get(3)?,
                started_at: NaiveDateTime::parse_from_str(
                    &r.get::<_, String>(4)?,
                    "%Y-%m-%d %H:%M:%S",
                )
                .unwrap(),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Normalize all activity names in `sessions` and `activity_archive` tables.
    /// Call once on startup to clean up historical data.
    /// Uses a version flag to run only once.
    pub fn normalize_activities(&self) -> anyhow::Result<()> {
        let mut conn = self.conn.lock().unwrap();

        // Check if normalization has already been run
        let already_normalized: bool = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'activities_normalized'",
                [],
                |r| {
                    let val: String = r.get(0)?;
                    Ok(val == "true")
                },
            )
            .unwrap_or(false);

        if already_normalized {
            return Ok(());
        }

        // Use rusqlite transaction for proper RAII and rollback semantics
        let tx = conn.transaction()?;

        // Step 1: Normalize activities in sessions table
        let mut stmt = tx.prepare("SELECT DISTINCT activity FROM sessions")?;
        let activities: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        for original in activities {
            let normalized = crate::normalize::normalize_activity(&original);
            if normalized != original {
                tx.execute(
                    "UPDATE sessions SET activity = ?1 WHERE activity = ?2",
                    params![normalized, original],
                )?;
            }
        }

        // Step 2: Normalize activities in activity_archive table
        let mut stmt = tx.prepare("SELECT DISTINCT activity FROM activity_archive")?;
        let activities: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        for original in activities {
            let normalized = crate::normalize::normalize_activity(&original);
            if normalized != original {
                tx.execute(
                    "UPDATE activity_archive SET activity = ?1 WHERE activity = ?2",
                    params![normalized, original],
                )?;
            }
        }

        // Step 3: Merge duplicate rows in activity_archive that now have the same (user_id, week_label, activity)
        // Find groups with duplicates
        let mut stmt = tx.prepare(
            "SELECT user_id, week_label, activity, COUNT(*) as cnt
             FROM activity_archive
             GROUP BY user_id, week_label, activity
             HAVING cnt > 1",
        )?;
        let duplicates: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        // Prepare statements once for all duplicate groups
        let mut select_stmt = tx.prepare(
            "SELECT id, total_min FROM activity_archive
             WHERE user_id = ?1 AND week_label = ?2 AND activity = ?3
             ORDER BY id ASC",
        )?;
        let mut update_stmt = tx.prepare(
            "UPDATE activity_archive SET total_min = ?1 WHERE id = ?2"
        )?;
        let mut delete_stmt = tx.prepare(
            "DELETE FROM activity_archive WHERE id = ?1"
        )?;

        // For each duplicate group, keep the row with MIN(id), sum total_min into it, delete rest
        for (user_id, week_label, activity) in duplicates {
            // Get all ids and total_min for this group
            let rows: Vec<(i64, i64)> = select_stmt
                .query_map(params![&user_id, &week_label, &activity], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect();

            if rows.len() > 1 {
                let keep_id = rows[0].0;
                let total_sum: i64 = rows.iter().map(|(_, mins)| mins).sum();

                // Update the kept row with the sum
                update_stmt.execute(params![total_sum, keep_id])?;

                // Delete the duplicate rows
                for (id, _) in rows.iter().skip(1) {
                    delete_stmt.execute(params![id])?;
                }
            }
        }

        drop(select_stmt);
        drop(update_stmt);
        drop(delete_stmt);

        // Mark normalization as complete
        tx.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('activities_normalized', 'true')",
            [],
        )?;

        // Commit transaction
        tx.commit()?;

        Ok(())
    }

    /// Rename all of a user's sessions with `old_activity` to `new_activity`.
    /// In `sessions`: UPDATE activity for all rows matching (user_id, old_activity).
    /// In `activity_archive`: UPDATE activity, then merge any resulting duplicates
    /// by summing total_min for the same (user_id, week_label, new_activity).
    /// Returns (sessions_updated, archive_rows_merged) counts.
    pub fn rename_activity(&self, user_id: &str, old_activity: &str, new_activity: &str) -> anyhow::Result<(usize, usize)> {
        let mut conn = self.conn.lock().unwrap();

        // Check that the user actually has sessions or archive entries with old_activity
        let has_sessions: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sessions WHERE user_id = ?1 AND activity = ?2",
            params![user_id, old_activity],
            |r| r.get(0),
        )?;

        let has_archive: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM activity_archive WHERE user_id = ?1 AND activity = ?2",
            params![user_id, old_activity],
            |r| r.get(0),
        )?;

        if !has_sessions && !has_archive {
            anyhow::bail!("no sessions found with that activity");
        }

        // Start transaction
        let tx = conn.transaction()?;

        // Update sessions table
        let sessions_updated = tx.execute(
            "UPDATE sessions SET activity = ?1 WHERE user_id = ?2 AND activity = ?3",
            params![new_activity, user_id, old_activity],
        )?;

        // Update activity_archive table
        tx.execute(
            "UPDATE activity_archive SET activity = ?1 WHERE user_id = ?2 AND activity = ?3",
            params![new_activity, user_id, old_activity],
        )?;

        // Merge duplicate archive rows for this user
        // Find groups with duplicates after the rename
        let mut stmt = tx.prepare(
            "SELECT user_id, week_label, activity, COUNT(*) as cnt
             FROM activity_archive
             WHERE user_id = ?1 AND activity = ?2
             GROUP BY user_id, week_label, activity
             HAVING cnt > 1",
        )?;
        let duplicates: Vec<(String, String, String)> = stmt
            .query_map(params![user_id, new_activity], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        let mut archive_rows_merged = 0;

        // Prepare statements for merging duplicates
        let mut select_stmt = tx.prepare(
            "SELECT id, total_min FROM activity_archive
             WHERE user_id = ?1 AND week_label = ?2 AND activity = ?3
             ORDER BY id ASC",
        )?;
        let mut update_stmt = tx.prepare(
            "UPDATE activity_archive SET total_min = ?1 WHERE id = ?2"
        )?;
        let mut delete_stmt = tx.prepare(
            "DELETE FROM activity_archive WHERE id = ?1"
        )?;

        // For each duplicate group, keep the row with MIN(id), sum total_min into it, delete rest
        for (uid, week_label, activity) in duplicates {
            let rows: Vec<(i64, i64)> = select_stmt
                .query_map(params![&uid, &week_label, &activity], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect();

            if rows.len() > 1 {
                let keep_id = rows[0].0;
                let total_sum: i64 = rows.iter().map(|(_, mins)| mins).sum();

                // Update the kept row with the sum
                update_stmt.execute(params![total_sum, keep_id])?;

                // Delete the duplicate rows
                for (id, _) in rows.iter().skip(1) {
                    delete_stmt.execute(params![id])?;
                    archive_rows_merged += 1;
                }
            }
        }

        drop(select_stmt);
        drop(update_stmt);
        drop(delete_stmt);

        // Commit transaction
        tx.commit()?;

        Ok((sessions_updated, archive_rows_merged))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_db() -> (Db, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Db::open(&db_path).unwrap();
        (db, temp_dir)
    }

    #[test]
    fn test_rename_activity_basic() {
        let (db, _temp_dir) = setup_test_db();
        let user_id = "user123";
        let username = "TestUser";

        // Clock in and out for "boring work"
        db.clock_in(user_id, username, "boring work").unwrap();
        let session = db.active_session(user_id).unwrap().unwrap();
        assert_eq!(session.activity, "boring work");
        
        // Clock out
        db.clock_out(user_id).unwrap();

        // Rename "boring work" to "work"
        let (sessions_updated, archive_merged) = db.rename_activity(user_id, "boring work", "work").unwrap();
        assert_eq!(sessions_updated, 1);
        assert_eq!(archive_merged, 0);

        // Verify the rename worked
        let conn = db.conn.lock().unwrap();
        let activity: String = conn
            .query_row(
                "SELECT activity FROM sessions WHERE user_id = ?1",
                params![user_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(activity, "work");
    }

    #[test]
    fn test_rename_activity_not_found() {
        let (db, _temp_dir) = setup_test_db();
        let user_id = "user123";

        // Try to rename a non-existent activity
        let result = db.rename_activity(user_id, "nonexistent", "work");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "no sessions found with that activity");
    }

    #[test]
    fn test_rename_activity_merge_archives() {
        let (db, _temp_dir) = setup_test_db();
        let user_id = "user123";
        let username = "TestUser";
        let week_label = "KW07/2026";

        // Manually insert archive entries for the same user and week but different activities
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO activity_archive (user_id, username, week_label, activity, total_min) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![user_id, username, week_label, "work", 60],
            ).unwrap();
            conn.execute(
                "INSERT INTO activity_archive (user_id, username, week_label, activity, total_min) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![user_id, username, week_label, "boring work", 30],
            ).unwrap();
        }

        // Rename "boring work" to "work" - should merge the archives
        let (sessions_updated, archive_merged) = db.rename_activity(user_id, "boring work", "work").unwrap();
        assert_eq!(sessions_updated, 0); // No sessions to update
        assert_eq!(archive_merged, 1); // One duplicate row merged

        // Verify the archives were merged
        let conn = db.conn.lock().unwrap();
        let total_min: i64 = conn
            .query_row(
                "SELECT total_min FROM activity_archive WHERE user_id = ?1 AND week_label = ?2 AND activity = ?3",
                params![user_id, week_label, "work"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total_min, 90); // 60 + 30

        // Verify only one row exists for this user/week/activity
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activity_archive WHERE user_id = ?1 AND week_label = ?2 AND activity = ?3",
                params![user_id, week_label, "work"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_rename_activity_active_session() {
        let (db, _temp_dir) = setup_test_db();
        let user_id = "user123";
        let username = "TestUser";

        // Clock in to "boring work"
        db.clock_in(user_id, username, "boring work").unwrap();
        
        // Rename while still clocked in
        let (sessions_updated, _) = db.rename_activity(user_id, "boring work", "work").unwrap();
        assert_eq!(sessions_updated, 1);

        // Verify the active session was renamed
        let session = db.active_session(user_id).unwrap().unwrap();
        assert_eq!(session.activity, "work");
    }

    #[test]
    fn test_rename_activity_per_user() {
        let (db, _temp_dir) = setup_test_db();
        let user1 = "user123";
        let user2 = "user456";
        let username1 = "User1";
        let username2 = "User2";

        // Both users have "boring work" sessions
        db.clock_in(user1, username1, "boring work").unwrap();
        db.clock_out(user1).unwrap();
        
        db.clock_in(user2, username2, "boring work").unwrap();
        db.clock_out(user2).unwrap();

        // User1 renames their activity
        let (sessions_updated, _) = db.rename_activity(user1, "boring work", "work").unwrap();
        assert_eq!(sessions_updated, 1);

        // Verify user1's activity was renamed but user2's wasn't
        let conn = db.conn.lock().unwrap();
        let user1_activity: String = conn
            .query_row(
                "SELECT activity FROM sessions WHERE user_id = ?1",
                params![user1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(user1_activity, "work");

        let user2_activity: String = conn
            .query_row(
                "SELECT activity FROM sessions WHERE user_id = ?1",
                params![user2],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(user2_activity, "boring work");
    }
}
