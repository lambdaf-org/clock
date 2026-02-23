use chrono::{Datelike, Duration, NaiveDateTime, Utc};
use chrono_tz::Europe::Zurich;
use sqlx::any::AnyPoolOptions;
use sqlx::{Any, Pool, Row};

pub struct Db {
    pool: Pool<Any>,
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

#[derive(Debug)]
pub struct UserActivityEntry {
    pub user_id: String,
    pub username: String,
    pub activity: String,
    pub total_minutes: i64,
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
    pub async fn open(database_url: &str) -> anyhow::Result<Self> {
        sqlx::any::install_default_drivers();
        let pool = AnyPoolOptions::new().connect(database_url).await?;

        let is_postgres = database_url.starts_with("postgres");

        let pk_type = if is_postgres {
            "BIGSERIAL PRIMARY KEY"
        } else {
            "INTEGER PRIMARY KEY AUTOINCREMENT"
        };

        let ddl = format!(
            "CREATE TABLE IF NOT EXISTS sessions (
                id          {pk},
                user_id     TEXT    NOT NULL,
                username    TEXT    NOT NULL,
                activity    TEXT    NOT NULL,
                started_at  TEXT    NOT NULL,
                ended_at    TEXT,
                minutes     INTEGER
            )",
            pk = pk_type
        );
        sqlx::query(&ddl).execute(&pool).await?;

        let ddl2 = format!(
            "CREATE TABLE IF NOT EXISTS weekly_archive (
                id          {pk},
                user_id     TEXT    NOT NULL,
                username    TEXT    NOT NULL,
                week_label  TEXT    NOT NULL,
                total_min   INTEGER NOT NULL
            )",
            pk = pk_type
        );
        sqlx::query(&ddl2).execute(&pool).await?;

        let ddl3 = format!(
            "CREATE TABLE IF NOT EXISTS activity_archive (
                id          {pk},
                user_id     TEXT    NOT NULL,
                username    TEXT    NOT NULL,
                week_label  TEXT    NOT NULL,
                activity    TEXT    NOT NULL,
                total_min   INTEGER NOT NULL
            )",
            pk = pk_type
        );
        sqlx::query(&ddl3).execute(&pool).await?;

        let ddl4 = "CREATE TABLE IF NOT EXISTS metadata (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )";
        sqlx::query(ddl4).execute(&pool).await?;

        let ddl5 = format!(
            "CREATE TABLE IF NOT EXISTS user_aliases (
                id          {pk},
                user_id     TEXT NOT NULL,
                keyword     TEXT NOT NULL,
                activity    TEXT NOT NULL,
                UNIQUE(user_id, keyword)
            )",
            pk = pk_type
        );
        sqlx::query(&ddl5).execute(&pool).await?;

        let ddl6 = format!(
            "CREATE TABLE IF NOT EXISTS global_aliases (
                id          {pk},
                keyword     TEXT NOT NULL UNIQUE,
                activity    TEXT NOT NULL
            )",
            pk = pk_type
        );
        sqlx::query(&ddl6).execute(&pool).await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sess_user ON sessions(user_id)")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sess_end ON sessions(ended_at)")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_arch_user ON weekly_archive(user_id)")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_actarch_user ON activity_archive(user_id)")
            .execute(&pool)
            .await
            .ok();

        Ok(Self { pool })
    }

    pub async fn clock_in(
        &self,
        user_id: &str,
        username: &str,
        activity: &str,
    ) -> anyhow::Result<()> {
        let row = sqlx::query(
            "SELECT COUNT(*) as cnt FROM sessions WHERE user_id = $1 AND ended_at IS NULL",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        let count: i64 = row.get("cnt");
        if count > 0 {
            anyhow::bail!("already clocked in");
        }
        sqlx::query("INSERT INTO sessions (user_id, username, activity, started_at) VALUES ($1, $2, $3, $4)")
            .bind(user_id)
            .bind(username)
            .bind(activity)
            .bind(now_ch_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clock_out(&self, user_id: &str) -> anyhow::Result<(i64, String)> {
        let row = sqlx::query(
            "SELECT id, started_at, activity FROM sessions WHERE user_id = $1 AND ended_at IS NULL",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => {
                let id: i64 = r.get("id");
                let started_str: String = r.get("started_at");
                let activity: String = r.get("activity");
                let started = NaiveDateTime::parse_from_str(&started_str, "%Y-%m-%d %H:%M:%S")?;
                let now = now_ch();
                let minutes = (now - started).num_minutes();
                sqlx::query("UPDATE sessions SET ended_at = $1, minutes = $2 WHERE id = $3")
                    .bind(now_ch_str())
                    .bind(minutes)
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok((minutes, activity))
            }
            None => anyhow::bail!("not clocked in"),
        }
    }

    pub async fn active_session(&self, user_id: &str) -> anyhow::Result<Option<ActiveSession>> {
        let row = sqlx::query(
            "SELECT id, user_id, username, activity, started_at FROM sessions WHERE user_id = $1 AND ended_at IS NULL",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            let started_str: String = r.get("started_at");
            ActiveSession {
                id: r.get("id"),
                user_id: r.get("user_id"),
                username: r.get("username"),
                activity: r.get("activity"),
                started_at: NaiveDateTime::parse_from_str(&started_str, "%Y-%m-%d %H:%M:%S")
                    .unwrap(),
            }
        }))
    }

    pub async fn leaderboard_weekly(&self) -> anyhow::Result<Vec<LeaderboardEntry>> {
        let monday = monday_of_current_week();
        let rows = sqlx::query(
            "SELECT MAX(username) as username, SUM(minutes) as total
             FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= $1
             GROUP BY user_id ORDER BY total DESC LIMIT 15",
        )
        .bind(&monday)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| LeaderboardEntry {
                username: r.get("username"),
                total_minutes: r.get("total"),
            })
            .collect())
    }

    pub async fn leaderboard_alltime(&self) -> anyhow::Result<Vec<LeaderboardEntry>> {
        let rows = sqlx::query(
            "SELECT MAX(username) as username, SUM(mins) as total FROM (
                SELECT user_id, username, SUM(minutes) as mins FROM sessions
                    WHERE ended_at IS NOT NULL GROUP BY user_id, username
                UNION ALL
                SELECT user_id, username, SUM(total_min) as mins FROM weekly_archive
                    GROUP BY user_id, username
             ) sub GROUP BY user_id ORDER BY total DESC LIMIT 15",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| LeaderboardEntry {
                username: r.get("username"),
                total_minutes: r.get("total"),
            })
            .collect())
    }

    pub async fn archive_week(&self, week_label: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO weekly_archive (user_id, username, week_label, total_min)
             SELECT user_id, MAX(username), $1, SUM(minutes) FROM sessions
             WHERE ended_at IS NOT NULL GROUP BY user_id",
        )
        .bind(week_label)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "INSERT INTO activity_archive (user_id, username, week_label, activity, total_min)
             SELECT user_id, MAX(username), $1, activity, SUM(minutes) FROM sessions
             WHERE ended_at IS NOT NULL GROUP BY user_id, activity",
        )
        .bind(week_label)
        .execute(&self.pool)
        .await?;

        sqlx::query("DELETE FROM sessions WHERE ended_at IS NOT NULL")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn activity_breakdown_weekly(&self) -> anyhow::Result<Vec<ActivityEntry>> {
        let monday = monday_of_current_week();
        let rows = sqlx::query(
            "SELECT MAX(username) as username, activity, SUM(minutes) as total, COUNT(*) as sessions
             FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= $1
             GROUP BY user_id, activity
             ORDER BY username ASC, total DESC",
        )
        .bind(&monday)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| ActivityEntry {
                username: r.get("username"),
                activity: r.get("activity"),
                total_minutes: r.get("total"),
                session_count: r.get("sessions"),
            })
            .collect())
    }

    pub async fn activity_breakdown_alltime(&self) -> anyhow::Result<Vec<ActivityEntry>> {
        let rows = sqlx::query(
            "SELECT MAX(username) as username, activity, SUM(mins) as total, SUM(cnt) as sessions FROM (
                SELECT user_id, username, activity, SUM(minutes) as mins, COUNT(*) as cnt
                    FROM sessions WHERE ended_at IS NOT NULL
                    GROUP BY user_id, activity
                UNION ALL
                SELECT user_id, username, activity, SUM(total_min) as mins, 0 as cnt
                    FROM activity_archive
                    GROUP BY user_id, activity
             ) sub GROUP BY user_id, activity ORDER BY username ASC, total DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| ActivityEntry {
                username: r.get("username"),
                activity: r.get("activity"),
                total_minutes: r.get("total"),
                session_count: r.get("sessions"),
            })
            .collect())
    }

    pub async fn weekly_summary(&self) -> anyhow::Result<WeeklySummary> {
        let monday = monday_of_current_week();

        let totals = sqlx::query(
            "SELECT COALESCE(SUM(minutes),0) as total_min, COUNT(*) as total_sessions, COUNT(DISTINCT user_id) as unique_workers
             FROM sessions WHERE ended_at IS NOT NULL AND started_at >= $1",
        )
        .bind(&monday)
        .fetch_one(&self.pool)
        .await?;
        let total_minutes: i64 = totals.get("total_min");
        let total_sessions: i64 = totals.get("total_sessions");
        let unique_workers: i64 = totals.get("unique_workers");

        let mvp = sqlx::query(
            "SELECT MAX(username) as username, SUM(minutes) as total FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= $1
             GROUP BY user_id ORDER BY total DESC LIMIT 1",
        )
        .bind(&monday)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| (r.get::<String, _>("username"), r.get::<i64, _>("total")));

        let top_activity = sqlx::query(
            "SELECT activity, SUM(minutes) as total FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= $1
             GROUP BY activity ORDER BY total DESC LIMIT 1",
        )
        .bind(&monday)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| (r.get::<String, _>("activity"), r.get::<i64, _>("total")));

        let longest_session = sqlx::query(
            "SELECT username, activity, minutes FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= $1
             ORDER BY minutes DESC LIMIT 1",
        )
        .bind(&monday)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| {
            (
                r.get::<String, _>("username"),
                r.get::<String, _>("activity"),
                r.get::<i64, _>("minutes"),
            )
        });

        let breakdown_rows = sqlx::query(
            "SELECT MAX(username) as username, activity, SUM(minutes) as total
             FROM sessions WHERE ended_at IS NOT NULL AND started_at >= $1
             GROUP BY user_id, activity ORDER BY username ASC, total DESC",
        )
        .bind(&monday)
        .fetch_all(&self.pool)
        .await?;
        let breakdown: Vec<ActivityEntry> = breakdown_rows
            .iter()
            .map(|r| ActivityEntry {
                username: r.get("username"),
                activity: r.get("activity"),
                total_minutes: r.get("total"),
                session_count: 0,
            })
            .collect();

        Ok(WeeklySummary {
            total_minutes,
            total_sessions,
            unique_workers,
            mvp,
            top_activity,
            longest_session,
            breakdown,
        })
    }

    pub async fn who_is_working(&self) -> anyhow::Result<Vec<ActiveSession>> {
        let rows = sqlx::query(
            "SELECT id, user_id, username, activity, started_at FROM sessions WHERE ended_at IS NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let started_str: String = r.get("started_at");
                ActiveSession {
                    id: r.get("id"),
                    user_id: r.get("user_id"),
                    username: r.get("username"),
                    activity: r.get("activity"),
                    started_at: NaiveDateTime::parse_from_str(&started_str, "%Y-%m-%d %H:%M:%S")
                        .unwrap(),
                }
            })
            .collect())
    }

    pub async fn user_activity_breakdown_weekly(&self) -> anyhow::Result<Vec<UserActivityEntry>> {
        let monday = monday_of_current_week();
        let rows = sqlx::query(
            "SELECT user_id, MAX(username) as username, activity, SUM(minutes) as total
             FROM sessions
             WHERE ended_at IS NOT NULL AND started_at >= $1
             GROUP BY user_id, activity
             ORDER BY user_id ASC, total DESC",
        )
        .bind(&monday)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| UserActivityEntry {
                user_id: r.get("user_id"),
                username: r.get("username"),
                activity: r.get("activity"),
                total_minutes: r.get("total"),
            })
            .collect())
    }

    /// Normalize all activity names in `sessions` and `activity_archive` tables.
    pub async fn normalize_activities(&self) -> anyhow::Result<()> {
        let already_normalized = sqlx::query("SELECT value FROM metadata WHERE key = $1")
            .bind("activities_normalized")
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get::<String, _>("value") == "true")
            .unwrap_or(false);

        if already_normalized {
            return Ok(());
        }

        let rows = sqlx::query("SELECT DISTINCT activity FROM sessions")
            .fetch_all(&self.pool)
            .await?;
        for row in &rows {
            let original: String = row.get("activity");
            let normalized = crate::normalize::normalize_activity(&original);
            if normalized != original {
                sqlx::query("UPDATE sessions SET activity = $1 WHERE activity = $2")
                    .bind(&normalized)
                    .bind(&original)
                    .execute(&self.pool)
                    .await?;
            }
        }

        let rows = sqlx::query("SELECT DISTINCT activity FROM activity_archive")
            .fetch_all(&self.pool)
            .await?;
        for row in &rows {
            let original: String = row.get("activity");
            let normalized = crate::normalize::normalize_activity(&original);
            if normalized != original {
                sqlx::query("UPDATE activity_archive SET activity = $1 WHERE activity = $2")
                    .bind(&normalized)
                    .bind(&original)
                    .execute(&self.pool)
                    .await?;
            }
        }

        let dupes = sqlx::query(
            "SELECT user_id, week_label, activity, COUNT(*) as cnt
             FROM activity_archive
             GROUP BY user_id, week_label, activity
             HAVING COUNT(*) > 1",
        )
        .fetch_all(&self.pool)
        .await?;

        for dupe in &dupes {
            let user_id: String = dupe.get("user_id");
            let week_label: String = dupe.get("week_label");
            let activity: String = dupe.get("activity");

            let group = sqlx::query(
                "SELECT id, total_min FROM activity_archive
                 WHERE user_id = $1 AND week_label = $2 AND activity = $3
                 ORDER BY id ASC",
            )
            .bind(&user_id)
            .bind(&week_label)
            .bind(&activity)
            .fetch_all(&self.pool)
            .await?;

            if group.len() > 1 {
                let keep_id: i64 = group[0].get("id");
                let total_sum: i64 = group.iter().map(|r| r.get::<i64, _>("total_min")).sum();

                sqlx::query("UPDATE activity_archive SET total_min = $1 WHERE id = $2")
                    .bind(total_sum)
                    .bind(keep_id)
                    .execute(&self.pool)
                    .await?;

                for row in group.iter().skip(1) {
                    let id: i64 = row.get("id");
                    sqlx::query("DELETE FROM activity_archive WHERE id = $1")
                        .bind(id)
                        .execute(&self.pool)
                        .await?;
                }
            }
        }

        sqlx::query("DELETE FROM metadata WHERE key = $1")
            .bind("activities_normalized")
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO metadata (key, value) VALUES ($1, $2)")
            .bind("activities_normalized")
            .bind("true")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn rename_activity(
        &self,
        user_id: &str,
        old_activity: &str,
        new_activity: &str,
    ) -> anyhow::Result<(u64, u64)> {
        let has_sessions: i64 = sqlx::query(
            "SELECT COUNT(*) as cnt FROM sessions WHERE user_id = $1 AND activity = $2",
        )
        .bind(user_id)
        .bind(old_activity)
        .fetch_one(&self.pool)
        .await?
        .get("cnt");

        let has_archive: i64 = sqlx::query(
            "SELECT COUNT(*) as cnt FROM activity_archive WHERE user_id = $1 AND activity = $2",
        )
        .bind(user_id)
        .bind(old_activity)
        .fetch_one(&self.pool)
        .await?
        .get("cnt");

        if has_sessions == 0 && has_archive == 0 {
            anyhow::bail!("no sessions found with that activity");
        }

        let sessions_result =
            sqlx::query("UPDATE sessions SET activity = $1 WHERE user_id = $2 AND activity = $3")
                .bind(new_activity)
                .bind(user_id)
                .bind(old_activity)
                .execute(&self.pool)
                .await?;
        let sessions_updated = sessions_result.rows_affected();

        sqlx::query(
            "UPDATE activity_archive SET activity = $1 WHERE user_id = $2 AND activity = $3",
        )
        .bind(new_activity)
        .bind(user_id)
        .bind(old_activity)
        .execute(&self.pool)
        .await?;

        let dupes = sqlx::query(
            "SELECT user_id, week_label, activity, COUNT(*) as cnt
             FROM activity_archive
             WHERE user_id = $1 AND activity = $2
             GROUP BY user_id, week_label, activity
             HAVING COUNT(*) > 1",
        )
        .bind(user_id)
        .bind(new_activity)
        .fetch_all(&self.pool)
        .await?;

        let mut archive_rows_merged: u64 = 0;

        for dupe in &dupes {
            let uid: String = dupe.get("user_id");
            let week_label: String = dupe.get("week_label");
            let activity: String = dupe.get("activity");

            let group = sqlx::query(
                "SELECT id, total_min FROM activity_archive
                 WHERE user_id = $1 AND week_label = $2 AND activity = $3
                 ORDER BY id ASC",
            )
            .bind(&uid)
            .bind(&week_label)
            .bind(&activity)
            .fetch_all(&self.pool)
            .await?;

            if group.len() > 1 {
                let keep_id: i64 = group[0].get("id");
                let total_sum: i64 = group.iter().map(|r| r.get::<i64, _>("total_min")).sum();

                sqlx::query("UPDATE activity_archive SET total_min = $1 WHERE id = $2")
                    .bind(total_sum)
                    .bind(keep_id)
                    .execute(&self.pool)
                    .await?;

                for row in group.iter().skip(1) {
                    let id: i64 = row.get("id");
                    sqlx::query("DELETE FROM activity_archive WHERE id = $1")
                        .bind(id)
                        .execute(&self.pool)
                        .await?;
                    archive_rows_merged += 1;
                }
            }
        }

        Ok((sessions_updated, archive_rows_merged))
    }

    // ── Alias methods ──────────────────────────────────────────

    pub async fn get_user_alias(
        &self,
        user_id: &str,
        keyword: &str,
    ) -> anyhow::Result<Option<String>> {
        let row =
            sqlx::query("SELECT activity FROM user_aliases WHERE user_id = $1 AND keyword = $2")
                .bind(user_id)
                .bind(keyword)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.get("activity")))
    }

    pub async fn set_user_alias(
        &self,
        user_id: &str,
        keyword: &str,
        activity: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM user_aliases WHERE user_id = $1 AND keyword = $2")
            .bind(user_id)
            .bind(keyword)
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO user_aliases (user_id, keyword, activity) VALUES ($1, $2, $3)")
            .bind(user_id)
            .bind(keyword)
            .bind(activity)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_user_alias(&self, user_id: &str, keyword: &str) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM user_aliases WHERE user_id = $1 AND keyword = $2")
            .bind(user_id)
            .bind(keyword)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_user_aliases(&self, user_id: &str) -> anyhow::Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            "SELECT keyword, activity FROM user_aliases WHERE user_id = $1 ORDER BY keyword",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get("keyword"), r.get("activity")))
            .collect())
    }

    pub async fn get_global_alias(&self, keyword: &str) -> anyhow::Result<Option<String>> {
        let row = sqlx::query("SELECT activity FROM global_aliases WHERE keyword = $1")
            .bind(keyword)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get("activity")))
    }

    pub async fn set_global_alias(&self, keyword: &str, activity: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM global_aliases WHERE keyword = $1")
            .bind(keyword)
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO global_aliases (keyword, activity) VALUES ($1, $2)")
            .bind(keyword)
            .bind(activity)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_global_alias(&self, keyword: &str) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM global_aliases WHERE keyword = $1")
            .bind(keyword)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_global_aliases(&self) -> anyhow::Result<Vec<(String, String)>> {
        let rows = sqlx::query("SELECT keyword, activity FROM global_aliases ORDER BY keyword")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get("keyword"), r.get("activity")))
            .collect())
    }

    pub async fn resolve_alias(&self, user_id: &str, input: &str) -> anyhow::Result<String> {
        if let Some(activity) = self.get_user_alias(user_id, input).await? {
            return Ok(activity);
        }
        if let Some(activity) = self.get_global_alias(input).await? {
            return Ok(activity);
        }
        Ok(input.to_string())
    }

    pub async fn recent_activities(
        &self,
        user_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<String>> {
        let sessions_rows = sqlx::query(
            "SELECT DISTINCT activity, MAX(started_at) as last_used
             FROM sessions WHERE user_id = $1
             GROUP BY activity
             ORDER BY last_used DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        let mut activities: Vec<(String, String)> = sessions_rows
            .iter()
            .map(|r| {
                (
                    r.get::<String, _>("activity"),
                    r.get::<String, _>("last_used"),
                )
            })
            .collect();

        let archive_rows = sqlx::query(
            "SELECT DISTINCT activity, MAX(week_label) as last_week
             FROM activity_archive WHERE user_id = $1
             GROUP BY activity
             ORDER BY last_week DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        for row in &archive_rows {
            let activity: String = row.get("activity");
            let week: String = row.get("last_week");
            if !activities.iter().any(|(a, _)| a == &activity) {
                activities.push((activity, format!("archive-{}", week)));
            }
        }

        activities.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(activities.into_iter().take(limit).map(|(a, _)| a).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn setup_test_db() -> Db {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let url = format!("sqlite:file:test{}?mode=memory&cache=shared", id);
        Db::open(&url).await.unwrap()
    }

    #[tokio::test]
    async fn test_rename_activity_basic() {
        let db = setup_test_db().await;
        let user_id = "user123";
        let username = "TestUser";

        db.clock_in(user_id, username, "boring work").await.unwrap();
        let session = db.active_session(user_id).await.unwrap().unwrap();
        assert_eq!(session.activity, "boring work");

        db.clock_out(user_id).await.unwrap();

        let (sessions_updated, archive_merged) = db
            .rename_activity(user_id, "boring work", "work")
            .await
            .unwrap();
        assert_eq!(sessions_updated, 1);
        assert_eq!(archive_merged, 0);

        let row = sqlx::query("SELECT activity FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let activity: String = row.get("activity");
        assert_eq!(activity, "work");
    }

    #[tokio::test]
    async fn test_rename_activity_not_found() {
        let db = setup_test_db().await;
        let user_id = "user123";

        let result = db.rename_activity(user_id, "nonexistent", "work").await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "no sessions found with that activity"
        );
    }

    #[tokio::test]
    async fn test_rename_activity_merge_archives() {
        let db = setup_test_db().await;
        let user_id = "user123";
        let username = "TestUser";
        let week_label = "KW07/2026";

        sqlx::query(
            "INSERT INTO activity_archive (user_id, username, week_label, activity, total_min) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(user_id).bind(username).bind(week_label).bind("work").bind(60i64)
        .execute(&db.pool).await.unwrap();

        sqlx::query(
            "INSERT INTO activity_archive (user_id, username, week_label, activity, total_min) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(user_id).bind(username).bind(week_label).bind("boring work").bind(30i64)
        .execute(&db.pool).await.unwrap();

        let (sessions_updated, archive_merged) = db
            .rename_activity(user_id, "boring work", "work")
            .await
            .unwrap();
        assert_eq!(sessions_updated, 0);
        assert_eq!(archive_merged, 1);

        let row = sqlx::query(
            "SELECT total_min FROM activity_archive WHERE user_id = $1 AND week_label = $2 AND activity = $3",
        )
        .bind(user_id).bind(week_label).bind("work")
        .fetch_one(&db.pool).await.unwrap();
        let total_min: i64 = row.get("total_min");
        assert_eq!(total_min, 90);

        let row = sqlx::query(
            "SELECT COUNT(*) as cnt FROM activity_archive WHERE user_id = $1 AND week_label = $2 AND activity = $3",
        )
        .bind(user_id).bind(week_label).bind("work")
        .fetch_one(&db.pool).await.unwrap();
        let count: i64 = row.get("cnt");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_rename_activity_active_session() {
        let db = setup_test_db().await;
        let user_id = "user123";
        let username = "TestUser";

        db.clock_in(user_id, username, "boring work").await.unwrap();

        let (sessions_updated, _) = db
            .rename_activity(user_id, "boring work", "work")
            .await
            .unwrap();
        assert_eq!(sessions_updated, 1);

        let session = db.active_session(user_id).await.unwrap().unwrap();
        assert_eq!(session.activity, "work");
    }

    #[tokio::test]
    async fn test_rename_activity_per_user() {
        let db = setup_test_db().await;
        let user1 = "user123";
        let user2 = "user456";
        let username1 = "User1";
        let username2 = "User2";

        db.clock_in(user1, username1, "boring work").await.unwrap();
        db.clock_out(user1).await.unwrap();

        db.clock_in(user2, username2, "boring work").await.unwrap();
        db.clock_out(user2).await.unwrap();

        let (sessions_updated, _) = db
            .rename_activity(user1, "boring work", "work")
            .await
            .unwrap();
        assert_eq!(sessions_updated, 1);

        let row = sqlx::query("SELECT activity FROM sessions WHERE user_id = $1")
            .bind(user1)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("activity"), "work");

        let row = sqlx::query("SELECT activity FROM sessions WHERE user_id = $1")
            .bind(user2)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("activity"), "boring work");
    }
}
