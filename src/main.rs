mod commands;
mod db;
mod normalize;

use db::Db;
use dotenv::dotenv;
use serenity::all::*;
use serenity::async_trait;
use std::env;
use std::path::Path;
use std::sync::Arc;

struct Handler {
    db: Arc<Db>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }
        commands::handle_command(&ctx, &msg, &self.db).await;
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("[clock] {} is online", ready.user.name);

        if let Some(channel_id) = summary_channel_id() {
            let embed = CreateEmbed::new()
                .color(0x2ecc71)
                .title("✅ ClockBot Online")
                .description(format!(
                    "Summary channel verified.\nWeekly reports will post here every Monday 00:00.",
                ))
                .footer(CreateEmbedFooter::new(
                    db::now_ch().format("%d.%m.%Y %H:%M").to_string(),
                ));
            let _ = channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }
    }
}

fn summary_channel_id() -> Option<ChannelId> {
    env::var("SUMMARY_CHANNEL")
        .ok()
        .and_then(|s| s.parse().ok())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    let token = env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN missing");
    let db = Arc::new(Db::open(Path::new("/data/clock.db"))?);

    // Normalize all existing activity names in the database
    db.normalize_activities()?;
    println!("[clock] Activity names normalized");

    let db_clone = Arc::clone(&db);
    let token_clone = token.clone();
    tokio::spawn(async move {
        weekly_reset_loop(&db_clone, &token_clone).await;
    });

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler { db })
        .await
        .expect("Failed to build client");

    client.start().await.expect("Failed to start client");
    Ok(())
}

/// Every Monday 00:00 Swiss time:
/// 1. Post weekly summary to SUMMARY_CHANNEL
/// 2. Archive the week
/// 3. Clear completed sessions
async fn weekly_reset_loop(db: &Arc<Db>, token: &str) {
    use chrono::{Datelike, Duration, Weekday};
    use tokio::time::{sleep, Duration as TokioDuration};

    let summary_channel: Option<ChannelId> = env::var("SUMMARY_CHANNEL")
        .ok()
        .and_then(|s| s.parse().ok());

    let http = Arc::new(Http::new(token));

    loop {
        let now = db::now_ch();

        let days_until_monday = match now.weekday() {
            Weekday::Mon if now.time().hour() == 0 && now.time().minute() < 1 => 0,
            Weekday::Mon => 7,
            Weekday::Tue => 6,
            Weekday::Wed => 5,
            Weekday::Thu => 4,
            Weekday::Fri => 3,
            Weekday::Sat => 2,
            Weekday::Sun => 1,
        };

        let next_monday = (now + Duration::days(days_until_monday))
            .date()
            .and_hms_opt(0, 0, 0)
            .unwrap();

        let wait_secs = (next_monday - now).num_seconds().max(1) as u64;
        sleep(TokioDuration::from_secs(wait_secs)).await;

        let week_label = db::swiss_week_label();

        // Post weekly summary before archiving
        if let Some(channel_id) = summary_channel {
            match db.weekly_summary() {
                Ok(summary) if summary.total_sessions > 0 => {
                    let embeds = commands::build_weekly_summary_embeds(&summary, &week_label);
                    let mut msg = CreateMessage::new();
                    for embed in embeds {
                        msg = msg.embed(embed);
                    }
                    if let Err(e) = channel_id.send_message(&http, msg).await {
                        eprintln!("[clock] Failed to post summary: {e}");
                    } else {
                        println!("[clock] Posted weekly summary for {week_label}");
                    }
                }
                Ok(_) => println!("[clock] No sessions to summarize for {week_label}"),
                Err(e) => eprintln!("[clock] Summary query failed: {e}"),
            }
        }

        // Archive and clear
        match db.archive_week(&week_label) {
            Ok(()) => println!("[clock] Archived {week_label}"),
            Err(e) => eprintln!("[clock] Archive failed: {e}"),
        }

        sleep(TokioDuration::from_secs(120)).await;
    }
}

use chrono::Timelike;
