use chrono::{NaiveDateTime, Utc};
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

            CREATE INDEX IF NOT EXISTS idx_sessions_user   ON sessions(user_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_ended   ON sessions(ended_at);
            CREATE INDEX IF NOT EXISTS idx_archive_user     ON weekly_archive(user_id);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Start a clock-in. Returns Err if already clocked in.
    pub fn clock_in(&self, user_id: &str, username: &str, activity: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let active: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sessions WHERE user_id = ?1 AND ended_at IS NULL",
            params![user_id],
            |r| r.get(0),
        )?;
        if active {
            anyhow::bail!("already clocked in");
        }
        let now = Utc::now().naive_utc();
        conn.execute(
            "INSERT INTO sessions (user_id, username, activity, started_at) VALUES (?1, ?2, ?3, ?4)",
            params![user_id, username, activity, now.format("%Y-%m-%d %H:%M:%S").to_string()],
        )?;
        Ok(())
    }

    /// Stop the active session. Returns minutes worked + activity name.
    pub fn clock_out(&self, user_id: &str) -> anyhow::Result<(i64, String)> {
        let conn = self.conn.lock().unwrap();
        let session: Option<(i64, String, String)> = conn
            .query_row(
                "SELECT id, started_at, activity FROM sessions WHERE user_id = ?1 AND ended_at IS NULL",
                params![user_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();

        match session {
            Some((id, started_str, activity)) => {
                let started = NaiveDateTime::parse_from_str(&started_str, "%Y-%m-%d %H:%M:%S")?;
                let now = Utc::now().naive_utc();
                let minutes = (now - started).num_minutes();
                conn.execute(
                    "UPDATE sessions SET ended_at = ?1, minutes = ?2 WHERE id = ?3",
                    params![now.format("%Y-%m-%d %H:%M:%S").to_string(), minutes, id],
                )?;
                Ok((minutes, activity))
            }
            None => anyhow::bail!("not clocked in"),
        }
    }

    /// Get active session for a user.
    pub fn active_session(&self, user_id: &str) -> anyhow::Result<Option<ActiveSession>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, user_id, username, activity, started_at FROM sessions WHERE user_id = ?1 AND ended_at IS NULL",
            params![user_id],
            |r| {
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
            },
        );
        match result {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Weekly leaderboard: completed sessions this week (Mon-Sun).
    pub fn leaderboard_weekly(&self) -> anyhow::Result<Vec<LeaderboardEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT username, SUM(minutes) as total
             FROM sessions
             WHERE ended_at IS NOT NULL
               AND started_at >= date('now', 'weekday 1', '-7 days')
             GROUP BY user_id
             ORDER BY total DESC
             LIMIT 15",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(LeaderboardEntry {
                username: r.get(0)?,
                total_minutes: r.get(1)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// All-time leaderboard: all completed sessions + archived weeks.
    pub fn leaderboard_alltime(&self) -> anyhow::Result<Vec<LeaderboardEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT username, SUM(mins) as total FROM (
                SELECT username, SUM(minutes) as mins FROM sessions
                    WHERE ended_at IS NOT NULL
                    GROUP BY user_id
                UNION ALL
                SELECT username, SUM(total_min) as mins FROM weekly_archive
                    GROUP BY user_id
             ) GROUP BY username ORDER BY total DESC LIMIT 15",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(LeaderboardEntry {
                username: r.get(0)?,
                total_minutes: r.get(1)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Archive current week's data and clear sessions table.
    pub fn archive_week(&self, week_label: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO weekly_archive (user_id, username, week_label, total_min)
             SELECT user_id, username, ?1, SUM(minutes)
             FROM sessions
             WHERE ended_at IS NOT NULL
             GROUP BY user_id",
            params![week_label],
        )?;
        // Force-close any open sessions
        conn.execute("DELETE FROM sessions WHERE ended_at IS NOT NULL", [])?;
        Ok(())
    }

    /// Who is currently clocked in?
    pub fn who_is_working(&self) -> anyhow::Result<Vec<ActiveSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, username, activity, started_at FROM sessions WHERE ended_at IS NULL",
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
}
