use crate::db::{self, ActivityEntry, Db, LeaderboardEntry, WeeklySummary};
use serenity::all::*;
use std::sync::Arc;

const HELP: &str = r#"**Commands**
`/clock in <activity>` — start tracking
`/clock out` — stop tracking
`/clock status` — your session
`/clock who` — who's working
`/clock leaderboard` — weekly + all-time
`/clock stats` — activity breakdown
`/clock rename <old> > <new>` — rename + merge activity
`/clock help`"#;

const COLOR_GREEN: u32 = 0x2ecc71;
const COLOR_RED: u32 = 0xe74c3c;
const COLOR_BLUE: u32 = 0x5865f2;
const COLOR_GOLD: u32 = 0xf1c40f;
const COLOR_GRAY: u32 = 0x2f3136;
const COLOR_PURPLE: u32 = 0x9b59b6;
const COLOR_ORANGE: u32 = 0xe67e22;

const BAR_FULL: &str = "█";
const BAR_EMPTY: &str = "░";
const BAR_WIDTH: usize = 16;

pub async fn handle_command(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    if !msg.content.starts_with("/clock") {
        return;
    }

    let rest = msg.content.strip_prefix("/clock").unwrap().trim();

    if rest == "help" || rest.is_empty() {
        let _ = msg.reply(&ctx.http, HELP).await;
        return;
    }

    if rest.starts_with("in ") {
        let activity = rest.strip_prefix("in ").unwrap().trim();
        if activity.is_empty() {
            let _ = msg
                .reply(&ctx.http, "What are you working on? `/clock in <activity>`")
                .await;
            return;
        }
        let activity = crate::normalize::normalize_activity(activity);
        handle_clock_in(ctx, msg, db, &activity).await;
    } else if rest == "out" {
        handle_clock_out(ctx, msg, db).await;
    } else if rest == "status" {
        handle_status(ctx, msg, db).await;
    } else if rest == "who" {
        handle_who(ctx, msg, db).await;
    } else if rest == "leaderboard" || rest == "lb" {
        handle_leaderboard(ctx, msg, db).await;
    } else if rest == "stats" {
        handle_stats(ctx, msg, db).await;
    } else if rest.starts_with("rename ") {
        let args = rest.strip_prefix("rename ").unwrap().trim();
        handle_rename(ctx, msg, db, args).await;
    } else {
        let _ = msg.reply(&ctx.http, HELP).await;
    }
}

fn format_duration(minutes: i64) -> String {
    let h = minutes / 60;
    let m = minutes % 60;
    if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m", m)
    }
}

fn make_bar(minutes: i64, max_minutes: i64) -> String {
    let ratio = if max_minutes > 0 {
        (minutes as f64 / max_minutes as f64).min(1.0)
    } else {
        0.0
    };
    let filled = (ratio * BAR_WIDTH as f64).round() as usize;
    let empty = BAR_WIDTH - filled;
    format!("{}{}", BAR_FULL.repeat(filled), BAR_EMPTY.repeat(empty))
}

fn make_pie_slice(minutes: i64, total: i64) -> String {
    let pct = if total > 0 {
        (minutes as f64 / total as f64 * 100.0).round() as i64
    } else {
        0
    };
    let blocks = (pct as f64 / 10.0).round() as usize;
    format!("{} {}%", "▓".repeat(blocks.max(1)), pct)
}

fn format_board(entries: &[LeaderboardEntry]) -> String {
    if entries.is_empty() {
        return "*No data yet*".into();
    }

    let medals = ["🥇", "🥈", "🥉"];
    let max_min = entries.iter().map(|e| e.total_minutes).max().unwrap_or(1);

     let max_name_len = entries.iter().map(|e| e.username.len()).max().unwrap_or(8);

    let mut out = String::new();
    for (i, e) in entries.iter().enumerate() {
        let medal = if i < 3 { medals[i] } else { "▫️" };
        let bar = make_bar(e.total_minutes, max_min);
        let dur = format_duration(e.total_minutes);
        out += &format!("{} `{:<width$} {}` {}\n", medal, e.username, bar, dur, width = max_name_len);
    }
    out
}

fn format_activity_breakdown(entries: &[ActivityEntry]) -> String {
    if entries.is_empty() {
        return "*No data yet*".into();
    }

    let mut out = String::new();
    let mut current_user = String::new();

    for e in entries {
        if e.username != current_user {
            if !current_user.is_empty() {
                out += "\n";
            }
            out += &format!("👤 **{}**\n", e.username);
            current_user = e.username.clone();
        }

        let user_total: i64 = entries
            .iter()
            .filter(|a| a.username == e.username)
            .map(|a| a.total_minutes)
            .sum();

        let pie = make_pie_slice(e.total_minutes, user_total);
        out += &format!(
            "  `{}` {} — {}\n",
            pie,
            e.activity,
            format_duration(e.total_minutes)
        );
    }
    out
}

fn swiss_timestamp() -> String {
    db::now_ch().format("%d.%m.%Y %H:%M").to_string()
}

/// Build weekly summary embeds for auto-posting to a channel.
pub fn build_weekly_summary_embeds(summary: &WeeklySummary, week_label: &str) -> Vec<CreateEmbed> {
    let mut embeds = Vec::new();

    let mut desc = format!(
        "```\n  {} total  ·  {} sessions  ·  {} people\n```\n",
        format_duration(summary.total_minutes),
        summary.total_sessions,
        summary.unique_workers,
    );

    desc += "**━━━ Awards ━━━**\n\n";

    if let Some((ref name, mins)) = summary.mvp {
        desc += &format!("🏅 **MVP** — {} with {}\n", name, format_duration(mins));
    }
    if let Some((ref activity, mins)) = summary.top_activity {
        desc += &format!(
            "🔥 **Hot Topic** — {} ({})\n",
            activity,
            format_duration(mins)
        );
    }
    if let Some((ref name, ref activity, mins)) = summary.longest_session {
        desc += &format!(
            "🏋️ **Marathon** — {} on {} ({})\n",
            name,
            activity,
            format_duration(mins)
        );
    }

    embeds.push(
        CreateEmbed::new()
            .color(COLOR_ORANGE)
            .title(format!("📊 Weekly Report — {}", week_label))
            .description(desc)
            .footer(CreateEmbedFooter::new(swiss_timestamp())),
    );

    if !summary.breakdown.is_empty() {
        embeds.push(
            CreateEmbed::new()
                .color(COLOR_PURPLE)
                .title("🔍 Who worked on what")
                .description(format_activity_breakdown(&summary.breakdown)),
        );
    }

    embeds
}

// ── Command handlers ──────────────────────────────────────

async fn handle_clock_in(ctx: &Context, msg: &Message, db: &Arc<Db>, activity: &str) {
    let user_id = msg.author.id.to_string();
    let username = msg.author.display_name().to_string();

    match db.clock_in(&user_id, &username, activity) {
        Ok(()) => {
            let embed = CreateEmbed::new()
                .color(COLOR_GREEN)
                .title("🟢 Clocked In")
                .description(format!(
                    "**{}** started working on **{}**",
                    username, activity
                ))
                .footer(CreateEmbedFooter::new(format!(
                    "{} · /clock out when done",
                    swiss_timestamp()
                )));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        Err(_) => {
            let session = db.active_session(&user_id).ok().flatten();
            let desc = match session {
                Some(s) => format!("Already on **{}**\nUse `/clock out` first", s.activity),
                None => "Already clocked in. `/clock out` first.".into(),
            };
            let embed = CreateEmbed::new()
                .color(COLOR_RED)
                .title("⚠️ Already Clocked In")
                .description(desc);
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}

async fn handle_clock_out(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    let user_id = msg.author.id.to_string();
    let username = msg.author.display_name().to_string();

    match db.clock_out(&user_id) {
        Ok((minutes, activity)) => {
            let embed = CreateEmbed::new()
                .color(COLOR_RED)
                .title("🔴 Clocked Out")
                .description(format!("**{}** finished working on **{}**", username, activity))
                .field("Duration", format_duration(minutes), true)
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        Err(_) => {
            let embed = CreateEmbed::new()
                .color(COLOR_GRAY)
                .title("🤷 Not Clocked In")
                .description("Use `/clock in <activity>` first.");
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}

async fn handle_status(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    let user_id = msg.author.id.to_string();
    let username = msg.author.display_name().to_string();

    match db.active_session(&user_id) {
        Ok(Some(session)) => {
            let now = db::now_ch();
            let elapsed = (now - session.started_at).num_minutes();
            let started = session.started_at.format("%H:%M").to_string();

            let embed = CreateEmbed::new()
                .color(COLOR_GREEN)
                .title(format!("🟢 {} is working", username))
                .field("Activity", &session.activity, true)
                .field("Elapsed", format_duration(elapsed), true)
                .field("Since", &started, true)
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        _ => {
            let embed = CreateEmbed::new()
                .color(COLOR_GRAY)
                .title(format!("😴 {} is offline", username))
                .description("`/clock in <activity>`");
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}

async fn handle_who(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    match db.who_is_working() {
        Ok(sessions) if !sessions.is_empty() => {
            let now = db::now_ch();
            let mut lines = String::new();
            for (i, s) in sessions.iter().enumerate() {
                let elapsed = (now - s.started_at).num_minutes();
                lines += &format!(
                    "**{}.** {} — {} `{}`\n",
                    i + 1,
                    s.username,
                    s.activity,
                    format_duration(elapsed),
                );
            }
            let embed = CreateEmbed::new()
                .color(COLOR_BLUE)
                .title(format!("🔨 {} currently working", sessions.len()))
                .description(lines)
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        _ => {
            let embed = CreateEmbed::new()
                .color(COLOR_GRAY)
                .title("😴 Nobody working");
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}

async fn handle_leaderboard(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    let weekly = db.leaderboard_weekly().unwrap_or_default();
    let alltime = db.leaderboard_alltime().unwrap_or_default();

    let week_label = db::swiss_week_label();
    let weekly_text = format_board(&weekly);
    let alltime_text = format_board(&alltime);

    let weekly_total: i64 = weekly.iter().map(|e| e.total_minutes).sum();
    let alltime_total: i64 = alltime.iter().map(|e| e.total_minutes).sum();

    let embed = CreateEmbed::new()
        .color(COLOR_GOLD)
        .title("🏆 Leaderboard")
        .field(
            format!("📅 This Week ({})", week_label),
            format!(
                "{}\n*Total: {}*",
                weekly_text,
                format_duration(weekly_total)
            ),
            false,
        )
        .field("\u{200b}", "\u{200b}", false)
        .field(
            "⏳ All Time",
            format!(
                "{}\n*Total: {}*",
                alltime_text,
                format_duration(alltime_total)
            ),
            false,
        )
        .footer(CreateEmbedFooter::new(format!(
            "{} · Resets every Monday 00:00",
            swiss_timestamp()
        )));

    let _ = msg
        .channel_id
        .send_message(&ctx.http, CreateMessage::new().embed(embed))
        .await;
}

async fn handle_stats(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    let weekly = db.activity_breakdown_weekly().unwrap_or_default();
    let week_label = db::swiss_week_label();

    if weekly.is_empty() {
        let embed = CreateEmbed::new()
            .color(COLOR_GRAY)
            .title("📊 No activity data this week")
            .description("Clock in to start tracking.");
        let _ = msg
            .channel_id
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await;
        return;
    }

    let breakdown_text = format_activity_breakdown(&weekly);

    // Aggregate top activities across all users
    let mut activity_totals: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    for e in &weekly {
        *activity_totals.entry(e.activity.clone()).or_insert(0) += e.total_minutes;
    }
    let mut sorted: Vec<_> = activity_totals.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    let max_act = sorted.first().map(|(_, m)| *m).unwrap_or(1);
    let mut top_acts = String::new();
    for (act, mins) in sorted.iter().take(8) {
        let bar = make_bar(*mins, max_act);
        top_acts += &format!("`{}` {} — {}\n", bar, act, format_duration(*mins));
    }

    let embed = CreateEmbed::new()
        .color(COLOR_PURPLE)
        .title(format!("📊 Activity Stats — {}", week_label))
        .field("🔥 Top Activities", &top_acts, false)
        .field("\u{200b}", "\u{200b}", false)
        .field("👤 Per Person", &breakdown_text, false)
        .footer(CreateEmbedFooter::new(swiss_timestamp()));

    let _ = msg
        .channel_id
        .send_message(&ctx.http, CreateMessage::new().embed(embed))
        .await;
}

async fn handle_rename(ctx: &Context, msg: &Message, db: &Arc<Db>, args: &str) {
    let user_id = msg.author.id.to_string();

    // Split args on " > " or ">"
    let parts: Vec<&str> = if args.contains(" > ") {
        args.split(" > ").collect()
    } else if args.contains('>') {
        args.split('>').map(|s| s.trim()).collect()
    } else {
        vec![]
    };

    // Validate input
    if parts.len() != 2 || parts[0].trim().is_empty() || parts[1].trim().is_empty() {
        let embed = CreateEmbed::new()
            .color(COLOR_RED)
            .title("⚠️ Invalid Syntax")
            .description("Usage: `/clock rename <old activity> > <new activity>`")
            .footer(CreateEmbedFooter::new(swiss_timestamp()));
        let _ = msg
            .channel_id
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await;
        return;
    }

    let old_name = crate::normalize::normalize_activity(parts[0].trim());
    let new_name = crate::normalize::normalize_activity(parts[1].trim());

    // Check if they're the same after normalization
    if old_name == new_name {
        let embed = CreateEmbed::new()
            .color(COLOR_GRAY)
            .title("ℹ️ Already the Same")
            .description(format!(
                "**{}** and **{}** are already the same after normalization.",
                parts[0].trim(),
                parts[1].trim()
            ))
            .footer(CreateEmbedFooter::new(swiss_timestamp()));
        let _ = msg
            .channel_id
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await;
        return;
    }

    // Call db.rename_activity
    match db.rename_activity(&user_id, &old_name, &new_name) {
        Ok((sessions_updated, archive_rows_merged)) => {
            let mut details = String::new();
            if sessions_updated > 0 {
                details.push_str(&format!("✅ {} session(s) updated\n", sessions_updated));
            }
            if archive_rows_merged > 0 {
                details.push_str(&format!("🔀 {} archive row(s) merged\n", archive_rows_merged));
            }
            if details.is_empty() {
                details = "*No changes made*".to_string();
            }

            let embed = CreateEmbed::new()
                .color(COLOR_BLUE)
                .title("✏️ Activity Renamed")
                .description(format!("**{}** → **{}**", old_name, new_name))
                .field("Changes", details, false)
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        Err(_) => {
            let embed = CreateEmbed::new()
                .color(COLOR_RED)
                .title("⚠️ Activity Not Found")
                .description(format!("No sessions found for **{}**", old_name))
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}
