mod commands;
mod db;

use db::Db;
use dotenv::dotenv;
use serenity::all::*;
use serenity::async_trait;
use std::env;
use std::path::Path;
use std::sync::Arc;

use chrono::Timelike;

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    let token = env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN missing");
    let db = Arc::new(Db::open(Path::new("clock.db"))?);

    // Weekly archive loop
    let db_clone = Arc::clone(&db);
    tokio::spawn(async move {
        weekly_reset_loop(&db_clone).await;
    });

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler { db })
        .await
        .expect("Failed to build client");

    client.start().await.expect("Failed to start client");
    Ok(())
}

/// Runs forever. Every Monday at 00:00 UTC, archives the week and clears sessions.
async fn weekly_reset_loop(db: &Arc<Db>) {
    use chrono::{Datelike, Duration, Utc, Weekday};
    use tokio::time::{sleep, Duration as TokioDuration};

    loop {
        let now = Utc::now();
        let days_until_monday = match now.weekday() {
            Weekday::Mon => {
                if now.hour() == 0 && now.minute() < 1 {
                    0
                } else {
                    7
                }
            }
            Weekday::Tue => 6,
            Weekday::Wed => 5,
            Weekday::Thu => 4,
            Weekday::Fri => 3,
            Weekday::Sat => 2,
            Weekday::Sun => 1,
        };

        let next_monday = (now + Duration::days(days_until_monday))
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap();

        let wait_secs = (next_monday - now.naive_utc()).num_seconds().max(1) as u64;
        sleep(TokioDuration::from_secs(wait_secs)).await;

        let label = Utc::now().format("W%V-%Y").to_string();
        if let Err(e) = db.archive_week(&label) {
            eprintln!("Weekly archive failed: {e}");
        } else {
            println!("Archived week {label}");
        }

        // Sleep a bit to avoid double-trigger
        sleep(TokioDuration::from_secs(120)).await;
    }
}
