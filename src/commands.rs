use crate::db::{self, ActivityEntry, Db, LeaderboardEntry, WeeklySummary};
use serenity::all::*;
use std::sync::Arc;

const HELP: &str = r#"**Commands**
`/clock in <activity>` — start tracking
`/clock out` — stop tracking
`/clock switch <activity>` — switch to new activity
`/clock status` — your session
`/clock who` — who's working
`/clock leaderboard` — weekly + all-time
`/clock stats` — activity breakdown
`/clock alltime` — all-time activity stats for everyone
`/clock rename <old> > <new>` — rename + merge activity
`/clock chart [weeks] [totals|cumulative|both]` — line chart of top 5 weekly hours
`/clock help`"#;

const COLOR_GREEN: u32 = 0x2ecc71;
const COLOR_RED: u32 = 0xe74c3c;
const COLOR_BLUE: u32 = 0x5865f2;
const COLOR_GOLD: u32 = 0xf1c40f;
const COLOR_DARK: u32 = 0x0a0b0f;
const COLOR_PURPLE: u32 = 0x9b59b6;
const COLOR_PINK: u32 = 0xff2d6f;
const COLOR_CYAN: u32 = 0x00d4ff;
const COLOR_AMBER: u32 = 0xffa500;
const COLOR_VIOLET: u32 = 0xb34dff;

const BAR_FULL: &str = "▰";
const BAR_EMPTY: &str = "▱";
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
    } else if rest.starts_with("switch ") {
        let activity = rest.strip_prefix("switch ").unwrap().trim();
        if activity.is_empty() {
            let _ = msg
                .reply(&ctx.http, "Switch to what? `/clock switch <activity>`")
                .await;
            return;
        }
        let activity = crate::normalize::normalize_activity(activity);
        handle_switch(ctx, msg, db, &activity).await;
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
    } else if rest == "alltime" || rest == "all-time" || rest == "at" {
        handle_alltime(ctx, msg, db).await;
    } else if rest.starts_with("rename ") {
        let args = rest.strip_prefix("rename ").unwrap().trim();
        handle_rename(ctx, msg, db, args).await;
    } else if rest.starts_with("chart") {
        let args = rest.strip_prefix("chart").unwrap().trim();
        handle_chart(ctx, msg, db, args).await;
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
    make_bar_width(minutes, max_minutes, BAR_WIDTH)
}

fn make_bar_width(minutes: i64, max_minutes: i64, width: usize) -> String {
    let ratio = if max_minutes > 0 {
        (minutes as f64 / max_minutes as f64).min(1.0)
    } else {
        0.0
    };
    let filled = (ratio * width as f64).round() as usize;
    let empty = width - filled;
    format!("{}{}", BAR_FULL.repeat(filled), BAR_EMPTY.repeat(empty))
}

fn format_board(entries: &[LeaderboardEntry]) -> String {
    if entries.is_empty() {
        return "*no data yet*".into();
    }

    const LB_BAR_WIDTH: usize = 12;
    const MAX_NAME: usize = 12;
    const RANK_WIDTH: usize = 3;    // "#1 " .. "#9 " or "#10"
    const DUR_WIDTH: usize = 9;     // right-align durations, e.g. " 66h 20m"
    const ELLIPSIS: &str = "...";
    const ELLIPSIS_LEN: usize = 3;  // length of ELLIPSIS above

    let max_min = entries.iter().map(|e| e.total_minutes).max().unwrap_or(1);

    let mut lines: Vec<String> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let rank_str = format!("#{}", i + 1);
        // left-align rank to RANK_WIDTH (right-pads with spaces: "#1 ", "#10")
        let rank_padded = format!("{:<width$}", rank_str, width = RANK_WIDTH);

        // truncate long names with ELLIPSIS so monospace width stays stable
        let name: String = if e.username.chars().count() > MAX_NAME {
            format!("{}{}", e.username.chars().take(MAX_NAME - ELLIPSIS_LEN).collect::<String>(), ELLIPSIS)
        } else {
            e.username.clone()
        };
        // pad name to MAX_NAME chars (manual to handle multi-byte chars correctly)
        let name_char_len = name.chars().count();
        let name_padded = format!("{}{}", name, " ".repeat(MAX_NAME.saturating_sub(name_char_len)));

        let bar = make_bar_width(e.total_minutes, max_min, LB_BAR_WIDTH);

        let dur = format_duration(e.total_minutes);
        // right-align duration to DUR_WIDTH
        let dur_char_len = dur.chars().count();
        let dur_padded = format!("{}{}", " ".repeat(DUR_WIDTH.saturating_sub(dur_char_len)), dur);

        lines.push(format!("{}  {}  {}  {}", rank_padded, name_padded, bar, dur_padded));
    }

    format!("```\n{}\n```", lines.join("\n"))
}

fn format_activity_breakdown(entries: &[ActivityEntry]) -> String {
    if entries.is_empty() {
        return "*No data yet*".into();
    }

    // Precompute per-user totals once to avoid O(n²) scans.
    let mut user_totals: std::collections::HashMap<&str, i64> =
        std::collections::HashMap::new();
    for e in entries {
        *user_totals.entry(e.username.as_str()).or_insert(0) += e.total_minutes;
    }

    let mut out = String::new();
    let mut current_user = String::new();

    for e in entries {
        let user_total = *user_totals.get(e.username.as_str()).unwrap_or(&0);
        if e.username != current_user {
            if !current_user.is_empty() {
                out += "\n";
            }
            out += &format!("**{}** — {}\n", e.username, format_duration(user_total));
            current_user = e.username.clone();
        }

        let pct = if user_total > 0 {
            (e.total_minutes as f64 / user_total as f64 * 100.0).round() as i64
        } else {
            0
        };
        out += &format!(
            "    {} · {} ({}%)\n",
            e.activity,
            format_duration(e.total_minutes),
            pct
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

    desc += "**highlights**\n\n";

    if let Some((ref name, mins)) = summary.mvp {
        desc += &format!("most time · {} ({})\n", name, format_duration(mins));
    }
    if let Some((ref activity, mins)) = summary.top_activity {
        desc += &format!("top activity · {} ({})\n", activity, format_duration(mins));
    }
    if let Some((ref name, ref activity, mins)) = summary.longest_session {
        desc += &format!(
            "longest session · {} on {} ({})\n",
            name,
            activity,
            format_duration(mins)
        );
    }

    embeds.push(
        CreateEmbed::new()
            .color(COLOR_AMBER)
            .title(format!("weekly report · {}", week_label))
            .description(desc)
            .footer(CreateEmbedFooter::new(swiss_timestamp())),
    );

    if !summary.breakdown.is_empty() {
        embeds.push(
            CreateEmbed::new()
                .color(COLOR_PURPLE)
                .title("activity breakdown")
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
                .title("clocked in")
                .description(format!("{} started {}.", username, activity))
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
                .color(COLOR_PINK)
                .title("already clocked in")
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
                .color(COLOR_PINK)
                .title("clocked out")
                .description(format!("{} finished {}.", username, activity))
                .field("duration", format_duration(minutes), true)
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        Err(_) => {
            let embed = CreateEmbed::new()
                .color(COLOR_DARK)
                .title("not clocked in")
                .description("Use `/clock in <activity>` first.");
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}

async fn handle_switch(ctx: &Context, msg: &Message, db: &Arc<Db>, activity: &str) {
    let user_id = msg.author.id.to_string();
    let username = msg.author.display_name().to_string();

    let session = db.active_session(&user_id).ok().flatten();
    let was_clocked_in = session.is_some();
    let prev_activity = session.map(|s| s.activity);

    if was_clocked_in {
        let _ = db.clock_out(&user_id);
    }

    match db.clock_in(&user_id, &username, activity) {
        Ok(()) => {
            let desc = if let Some(prev) = prev_activity {
                format!(
                    "{} switched from {} to {}.",
                    username, prev, activity
                )
            } else {
                format!("{} started {}.", username, activity)
            };
            let embed = CreateEmbed::new()
                .color(COLOR_CYAN)
                .title("switched")
                .description(desc)
                .footer(CreateEmbedFooter::new(format!(
                    "{} · /clock out when done",
                    swiss_timestamp()
                )));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        Err(e) => {
            let embed = CreateEmbed::new()
                .color(COLOR_PINK)
                .title("switch failed")
                .description(format!("Error: {}", e));
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
                .title(format!("status · {}", username))
                .field("activity", &session.activity, true)
                .field("elapsed", format_duration(elapsed), true)
                .field("since", &started, true)
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        _ => {
            let embed = CreateEmbed::new()
                .color(COLOR_DARK)
                .title(format!("status · {}", username))
                .description("Not clocked in. Use `/clock in <activity>`.");
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
                    "{}. {} · {} · {}\n",
                    i + 1,
                    s.username,
                    s.activity,
                    format_duration(elapsed),
                );
            }
            let embed = CreateEmbed::new()
                .color(COLOR_BLUE)
                .title(format!("active now · {}", sessions.len()))
                .description(lines)
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        _ => {
            let embed = CreateEmbed::new()
                .color(COLOR_DARK)
                .title("active now")
                .description("No active sessions.");
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
    match crate::chart::render_leaderboard_card(&week_label, &weekly, &alltime) {
        Ok(png_bytes) => {
            let embed = CreateEmbed::new()
                .color(COLOR_DARK)
                .title("leaderboard")
                .description("Weekly and all-time overview")
                .image("attachment://leaderboard.png")
                .footer(CreateEmbedFooter::new(format!(
                    "{} · Resets every Monday 00:00",
                    swiss_timestamp()
                )));

            let attachment = CreateAttachment::bytes(png_bytes, "leaderboard.png");
            let _ = msg
                .channel_id
                .send_message(
                    &ctx.http,
                    CreateMessage::new().embed(embed).add_file(attachment),
                )
                .await;
        }
        Err(e) => {
            let weekly_text = format_board(&weekly);
            let alltime_text = format_board(&alltime);
            let weekly_total: i64 = weekly.iter().map(|entry| entry.total_minutes).sum();
            let alltime_total: i64 = alltime.iter().map(|entry| entry.total_minutes).sum();
            let embed = CreateEmbed::new()
                .color(COLOR_DARK)
                .title("leaderboard")
                .description(format!("Fallback view · {}", e))
                .field(
                    format!("this week · {}", week_label),
                    format!(
                        "{}\n*total · {}*",
                        weekly_text,
                        format_duration(weekly_total)
                    ),
                    false,
                )
                .field(
                    "all time",
                    format!(
                        "{}\n*total · {}*",
                        alltime_text,
                        format_duration(alltime_total)
                    ),
                    false,
                )
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}

async fn handle_stats(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    let weekly = db.activity_breakdown_weekly().unwrap_or_default();
    let week_label = db::swiss_week_label();

    if weekly.is_empty() {
        let embed = CreateEmbed::new()
            .color(COLOR_DARK)
            .title(format!("activity stats · {}", week_label))
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
        .color(COLOR_VIOLET)
        .title(format!("activity stats · {}", week_label))
        .field("top activities", &top_acts, false)
        .field("\u{200b}", "\u{200b}", false)
        .field("per person", &breakdown_text, false)
        .footer(CreateEmbedFooter::new(swiss_timestamp()));

    let _ = msg
        .channel_id
        .send_message(&ctx.http, CreateMessage::new().embed(embed))
        .await;
}

async fn handle_alltime(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    let alltime = match db.activity_breakdown_alltime() {
        Ok(data) => data,
        Err(_) => {
            let embed = CreateEmbed::new()
                .color(COLOR_RED)
                .title("all-time stats unavailable")
                .description("Please try again in a moment.");
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
            return;
        }
    };

    if alltime.is_empty() {
        let embed = CreateEmbed::new()
            .color(COLOR_DARK)
            .title("all-time stats")
            .description("Clock in to start tracking.");
        let _ = msg
            .channel_id
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await;
        return;
    }

    // Aggregate top activities and per-user totals across all users
    let mut activity_totals: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    let mut user_totals: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for e in &alltime {
        *activity_totals.entry(e.activity.clone()).or_insert(0) += e.total_minutes;
        *user_totals.entry(e.username.clone()).or_insert(0) += e.total_minutes;
    }

    let grand_total: i64 = user_totals.values().sum();
    let people_count = user_totals.len();

    // Sort top activities descending
    let mut sorted_acts: Vec<(String, i64)> = activity_totals.into_iter().collect();
    sorted_acts.sort_by(|a, b| b.1.cmp(&a.1));

    let max_act = sorted_acts.first().map(|(_, m)| *m).unwrap_or(1);

    // Build top-10 activities bar chart
    let mut top_acts = String::new();
    for (act, mins) in sorted_acts.iter().take(10) {
        let bar = make_bar(*mins, max_act);
        top_acts += &format!("`{}` {} — {}\n", bar, act, format_duration(*mins));
    }
    if top_acts.is_empty() {
        top_acts = "*No activities found*".to_string();
    }

    // Embed 1: overview + top activities
    let embed1 = CreateEmbed::new()
        .color(COLOR_GOLD)
        .title("all-time activity stats")
        .description(format!(
            "```\n  {} total across {} people\n```",
            format_duration(grand_total),
            people_count,
        ))
        .field("top activities", &top_acts, false)
        .footer(CreateEmbedFooter::new(swiss_timestamp()));

    // Embed 2: per-person breakdown (reuse existing formatter)
    let mut breakdown_text = format_activity_breakdown(&alltime);
    const MAX_EMBED_DESC_CHARS: usize = 4000;
    if breakdown_text.chars().count() > MAX_EMBED_DESC_CHARS {
        let keep = MAX_EMBED_DESC_CHARS.saturating_sub(16);
        let truncated: String = breakdown_text.chars().take(keep).collect();
        breakdown_text = format!("{truncated}\n… (truncated)");
    }
    let embed2 = CreateEmbed::new()
        .color(COLOR_PURPLE)
        .title("per-person breakdown")
        .description(breakdown_text);

    let _ = msg
        .channel_id
        .send_message(
            &ctx.http,
            CreateMessage::new().add_embeds(vec![embed1, embed2]),
        )
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
            .color(COLOR_PINK)
            .title("invalid syntax")
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
            .color(COLOR_DARK)
            .title("already the same")
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
                details.push_str(&format!("{} session(s) updated\n", sessions_updated));
            }
            if archive_rows_merged > 0 {
                details.push_str(&format!("{} archive row(s) merged\n", archive_rows_merged));
            }
            if details.is_empty() {
                details = "*No changes made*".to_string();
            }

            let embed = CreateEmbed::new()
                .color(COLOR_BLUE)
                .title("renamed")
                .description(format!("**{}** → **{}**", old_name, new_name))
                .field("changes", details, false)
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        Err(_) => {
            let embed = CreateEmbed::new()
                .color(COLOR_PINK)
                .title("activity not found")
                .description(format!("No sessions found for **{}**", old_name))
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}

async fn handle_chart(ctx: &Context, msg: &Message, db: &Arc<Db>, args: &str) {
    // Parse optional positional arguments: [weeks] [mode]
    let mut weeks: u32 = 12;
    let mut mode_str = "totals";

    for token in args.split_whitespace() {
        if let Ok(n) = token.parse::<u32>() {
            if (1..=52).contains(&n) {
                weeks = n;
            }
        } else if matches!(token, "totals" | "cumulative" | "both") {
            mode_str = token;
        }
    }

    let mode = crate::chart::ChartMode::from_str(mode_str);

    // Typing indicator while we render.
    let _ = msg.channel_id.broadcast_typing(&ctx.http).await;

    let data = match db.weekly_hours_for_chart(weeks) {
        Ok(d) => d,
        Err(e) => {
            let embed = CreateEmbed::new()
                .color(COLOR_DARK)
                .title("chart unavailable")
                .description(format!("{}", e))
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
            return;
        }
    };

    if data.users.is_empty() {
        let embed = CreateEmbed::new()
            .color(COLOR_DARK)
            .title("not enough data")
            .description(format!(
                "No time entries found in the last {} week(s).",
                weeks
            ))
            .footer(CreateEmbedFooter::new(swiss_timestamp()));
        let _ = msg
            .channel_id
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await;
        return;
    }

    if data.week_labels.len() < 2 {
        let embed = CreateEmbed::new()
            .color(COLOR_DARK)
            .title("not enough data")
            .description("Need at least 2 weeks of data to draw a chart.")
            .footer(CreateEmbedFooter::new(swiss_timestamp()));
        let _ = msg
            .channel_id
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await;
        return;
    }

    let png_bytes = match crate::chart::render_chart(&data, mode) {
        Ok(b) => b,
        Err(e) => {
            let embed = CreateEmbed::new()
                .color(COLOR_PINK)
                .title("chart render error")
                .description(format!("Failed to generate chart: {}", e))
                .footer(CreateEmbedFooter::new(swiss_timestamp()));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
            return;
        }
    };

    // Build a summary of the top users for the embed description.
    let first_week = data.week_labels.first().map(String::as_str).unwrap_or("?");
    let last_week = data.week_labels.last().map(String::as_str).unwrap_or("?");

    let mut user_summary = String::new();
    for (i, user) in data.users.iter().enumerate() {
        let total_min: i64 = user.minutes_per_week.iter().sum();
        user_summary += &format!(
            "{}. {} · {}\n",
            i + 1,
            user.username,
            format_duration(total_min)
        );
    }

    let embed = CreateEmbed::new()
        .color(COLOR_BLUE)
        .title(format!("chart · {}w", weeks))
        .description(format!(
            "{} → {} · {}\n\n{}",
            first_week, last_week, mode_str, user_summary
        ))
        .image("attachment://chart.png")
        .footer(CreateEmbedFooter::new(swiss_timestamp()));

    let attachment = CreateAttachment::bytes(png_bytes, "chart.png");
    let _ = msg
        .channel_id
        .send_message(
            &ctx.http,
            CreateMessage::new().embed(embed).add_file(attachment),
        )
        .await;
}
