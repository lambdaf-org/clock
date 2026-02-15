use crate::db::Db;
use chrono::Utc;
use serenity::all::*;
use std::sync::Arc;

const HELP: &str = r#"**⏱ ClockBot Commands**
`/clock in <what you're working on>`
`/clock out`
`/clock status`
`/clock who`
`/clock leaderboard`
`/clock help`"#;

const COLOR_GREEN: u32 = 0x2ecc71;
const COLOR_RED: u32 = 0xe74c3c;
const COLOR_BLUE: u32 = 0x5865f2;
const COLOR_GOLD: u32 = 0xf1c40f;
const COLOR_GRAY: u32 = 0x95a5a6;

pub async fn handle_command(ctx: &Context, msg: &Message, db: &Arc<Db>) {
    if !msg.content.starts_with("/clock") {
        return;
    }

    let content = msg.content.trim();
    let rest = content.strip_prefix("/clock").unwrap().trim();

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
        handle_clock_in(ctx, msg, db, activity).await;
    } else if rest == "out" {
        handle_clock_out(ctx, msg, db).await;
    } else if rest == "status" {
        handle_status(ctx, msg, db).await;
    } else if rest == "who" {
        handle_who(ctx, msg, db).await;
    } else if rest == "leaderboard" || rest == "lb" {
        handle_leaderboard(ctx, msg, db).await;
    } else {
        let _ = msg.reply(&ctx.http, HELP).await;
    }
}

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
                .timestamp(Timestamp::now())
                .footer(CreateEmbedFooter::new("Use /clock out when you're done"));
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        Err(_) => {
            let session = db.active_session(&user_id).ok().flatten();
            let desc = match session {
                Some(s) => format!(
                    "You're already clocked in on **{}**\nClock out first with `/clock out`",
                    s.activity
                ),
                None => "You're already clocked in. Clock out first.".into(),
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
            let (h, m) = (minutes / 60, minutes % 60);
            let duration = if h > 0 {
                format!("{}h {}m", h, m)
            } else {
                format!("{}m", m)
            };

            let embed = CreateEmbed::new()
                .color(COLOR_RED)
                .title("🔴 Clocked Out")
                .description(format!("**{}** finished **{}**", username, activity))
                .field("Duration", &duration, true)
                .timestamp(Timestamp::now());
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        Err(_) => {
            let embed = CreateEmbed::new()
                .color(COLOR_GRAY)
                .title("🤷 Not Clocked In")
                .description("You're not clocked in. Use `/clock in <activity>` first.");
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
            let now = Utc::now().naive_utc();
            let elapsed = (now - session.started_at).num_minutes();
            let (h, m) = (elapsed / 60, elapsed % 60);
            let duration = if h > 0 {
                format!("{}h {}m", h, m)
            } else {
                format!("{}m", m)
            };

            let embed = CreateEmbed::new()
                .color(COLOR_GREEN)
                .title(format!("🟢 {} is working", username))
                .field("Activity", &session.activity, true)
                .field("Elapsed", &duration, true)
                .timestamp(Timestamp::now());
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        _ => {
            let embed = CreateEmbed::new()
                .color(COLOR_GRAY)
                .title(format!("😴 {} is not working", username))
                .description("Clock in with `/clock in <activity>`");
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
            let now = Utc::now().naive_utc();
            let mut lines = String::new();
            for (i, s) in sessions.iter().enumerate() {
                let elapsed = (now - s.started_at).num_minutes();
                let (h, m) = (elapsed / 60, elapsed % 60);
                let dur = if h > 0 {
                    format!("{}h {}m", h, m)
                } else {
                    format!("{}m", m)
                };
                lines += &format!("**{}. {}** — {} `{}`\n", i + 1, s.username, s.activity, dur);
            }
            let embed = CreateEmbed::new()
                .color(COLOR_BLUE)
                .title(format!("🔨 {} people working", sessions.len()))
                .description(lines);
            let _ = msg
                .channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
        _ => {
            let embed = CreateEmbed::new()
                .color(COLOR_GRAY)
                .title("😴 Nobody is working right now");
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

    let medals = ["🥇", "🥈", "🥉"];

    let format_board = |entries: &[crate::db::LeaderboardEntry]| -> String {
        if entries.is_empty() {
            return "*No data yet*".into();
        }
        let mut out = String::new();
        for (i, e) in entries.iter().enumerate() {
            let prefix = if i < 3 { medals[i] } else { "▫️" };
            let (h, m) = (e.total_minutes / 60, e.total_minutes % 60);
            let dur = if h > 0 {
                format!("{}h {}m", h, m)
            } else {
                format!("{}m", m)
            };
            out += &format!("{} **{}** — `{}`\n", prefix, e.username, dur);
        }
        out
    };

    let weekly_text = format_board(&weekly);
    let alltime_text = format_board(&alltime);

    let embed = CreateEmbed::new()
        .color(COLOR_GOLD)
        .title("🏆 Leaderboard")
        .field("📅 This Week", &weekly_text, false)
        .field("\u{200b}", "\u{200b}", false) // spacer
        .field("⏳ All Time", &alltime_text, false)
        .timestamp(Timestamp::now())
        .footer(CreateEmbedFooter::new("Weekly stats reset every Monday"));

    let _ = msg
        .channel_id
        .send_message(&ctx.http, CreateMessage::new().embed(embed))
        .await;
}
