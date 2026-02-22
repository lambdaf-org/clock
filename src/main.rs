mod commands;
mod db;
mod normalize;
mod roles;

use db::Db;
use dotenv::dotenv;
use roles::RoleClassifier;
use serenity::all::*;
use serenity::async_trait;
use std::env;
use std::sync::Arc;

struct Handler {
    db: Arc<Db>,
    classifier: Arc<RoleClassifier>,
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
                .description(
                    "Summary channel verified.\nWeekly reports will post here every Monday 00:00.",
                )
                .footer(CreateEmbedFooter::new(
                    db::now_ch().format("%d.%m.%Y %H:%M").to_string(),
                ));
            let _ = channel_id
                .send_message(&ctx.http, CreateMessage::new().embed(embed))
                .await;
        }

        // ── Test run: assign roles immediately on startup ──
        if let Some(gid) = guild_id() {
            let db = Arc::clone(&self.db);
            let classifier = Arc::clone(&self.classifier);
            let http = Arc::new(ctx.http.clone());
            let channel = summary_channel_id();
            tokio::spawn(async move {
                println!("[roles] Test run: assigning roles on startup...");
                match assign_weekly_roles(&db, &classifier, &http, gid, channel).await {
                    Ok(count) => println!("[roles] Test run done. Assigned to {count} users."),
                    Err(e) => eprintln!("[roles] Test run failed: {e}"),
                }
            });
        }
    }
}

fn summary_channel_id() -> Option<ChannelId> {
    env::var("SUMMARY_CHANNEL")
        .ok()
        .and_then(|s| s.parse().ok())
}

fn guild_id() -> Option<GuildId> {
    env::var("GUILD_ID").ok().and_then(|s| s.parse().ok())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    let token = env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN missing");
    let db_url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:///data/clock.db".into());
    let db = Arc::new(Db::open(&db_url).await?);

    // Normalize all existing activity names in the database
    db.normalize_activities().await?;
    println!("[clock] Activity names normalized");

    // Load embedding model (downloads on first run, cached after)
    let classifier = Arc::new(RoleClassifier::new()?);

    let db_clone = Arc::clone(&db);
    let classifier_clone = Arc::clone(&classifier);
    let token_clone = token.clone();
    tokio::spawn(async move {
        weekly_reset_loop(&db_clone, &classifier_clone, &token_clone).await;
    });

    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler { db, classifier })
        .await
        .expect("Failed to build client");

    client.start().await.expect("Failed to start client");
    Ok(())
}

async fn create_role_above(
    http: &Arc<Http>,
    guild_id: GuildId,
    name: &str,
    colour: u32,
    above_role_id: RoleId,
) -> anyhow::Result<Role> {
    let roles = guild_id.roles(http).await?;
    let above = roles
        .get(&above_role_id)
        .ok_or_else(|| anyhow::anyhow!("Anchor role not found"))?;

    let role = guild_id
        .create_role(http, EditRole::new().name(name).colour(colour))
        .await?;

    guild_id
        .edit_role_position(http, role.id, above.position + 1)
        .await?;

    Ok(role)
}

async fn weekly_reset_loop(db: &Arc<Db>, classifier: &Arc<RoleClassifier>, token: &str) {
    use chrono::{Datelike, Duration, Timelike, Weekday};
    use tokio::time::{Duration as TokioDuration, sleep};

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

        // ── Assign roles before archiving (data still in sessions table) ──
        if let Some(gid) = guild_id() {
            match assign_weekly_roles(db, classifier, &http, gid, summary_channel).await {
                Ok(count) => println!("[roles] Assigned roles to {count} users"),
                Err(e) => eprintln!("[roles] Role assignment failed: {e}"),
            }
        }

        // ── Post summary ──
        if let Some(channel_id) = summary_channel {
            match db.weekly_summary().await {
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

        // ── Archive ──
        match db.archive_week(&week_label).await {
            Ok(()) => println!("[clock] Archived {week_label}"),
            Err(e) => eprintln!("[clock] Archive failed: {e}"),
        }

        sleep(TokioDuration::from_secs(120)).await;
    }
}

/// Check if a role name matches our generated role format.
/// Tier 2+: starts with 〔
/// Tier 1: plain word — we track these by storing assigned role IDs,
///         but for cleanup we only match 〔 prefix (tier 1 roles are too
///         ambiguous to match by name alone, so we delete all bot-created roles).
fn is_generated_role(name: &str) -> bool {
    name.starts_with('〔')
}

/// Delete all existing generated roles from the guild.
async fn cleanup_old_roles(http: &Arc<Http>, guild_id: GuildId) -> anyhow::Result<usize> {
    let guild_roles = guild_id.roles(http).await?;
    let mut count = 0;
    for (id, role) in &guild_roles {
        if is_generated_role(&role.name) {
            if let Err(e) = guild_id.delete_role(http, *id).await {
                eprintln!("[roles] Failed to delete old role '{}': {e}", role.name);
            } else {
                count += 1;
            }
        }
    }
    Ok(count)
}

/// Reset nicknames for all members who have chevron prefixes.
async fn reset_nicknames(http: &Arc<Http>, guild_id: GuildId) -> anyhow::Result<usize> {
    let members = guild_id.members(http, Some(1000), None).await?;
    let mut count = 0;
    for member in &members {
        if let Some(nick) = &member.nick {
            // Check if nickname contains our chevron marker
            if nick.contains('⟫') || nick.contains(" | ") {
                // Reset to no nickname (reverts to username)
                if let Err(e) = guild_id
                    .edit_member(http, member.user.id, EditMember::new().nickname(""))
                    .await
                {
                    eprintln!(
                        "[roles] Failed to reset nickname for {}: {e}",
                        member.user.name
                    );
                } else {
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

/// Build the chevron nickname for a user.
/// Tier 1: plain name (no chevrons)
/// Tier 2+: ⟫×(tier-1) | name
fn build_nickname(tier: usize, display_name: &str) -> String {
    let nick = if tier <= 1 {
        display_name.to_string()
    } else {
        let chevrons = "⟫".repeat(tier - 1);
        format!("{} | {}", chevrons, display_name)
    };

    // Discord nickname limit: 32 chars
    if nick.chars().count() > 32 {
        nick.chars().take(32).collect()
    } else {
        nick
    }
}

/// Assign Discord roles based on weekly activity.
/// 1. Reset all chevron nicknames
/// 2. Delete old generated roles
/// 3. Classify each user
/// 4. Create role + set nickname
async fn assign_weekly_roles(
    db: &Arc<Db>,
    classifier: &Arc<RoleClassifier>,
    http: &Arc<Http>,
    guild_id: GuildId,
    announce_channel: Option<ChannelId>,
) -> anyhow::Result<usize> {
    // Step 1: Reset nicknames
    match reset_nicknames(http, guild_id).await {
        Ok(n) => {
            if n > 0 {
                println!("[roles] Reset {n} nicknames");
            }
        }
        Err(e) => eprintln!("[roles] Nickname reset failed: {e}"),
    }

    // Step 2: Delete old roles
    match cleanup_old_roles(http, guild_id).await {
        Ok(n) => {
            if n > 0 {
                println!("[roles] Cleaned up {n} old roles");
            }
        }
        Err(e) => eprintln!("[roles] Cleanup failed: {e}"),
    }

    // Step 3: Gather activity data
    let _breakdown = db.activity_breakdown_weekly().await?;
    let user_activities = db.user_activity_breakdown_weekly().await?;

    let mut count = 0;

    // Group activities by user_id
    let mut per_user: std::collections::HashMap<String, Vec<(String, i64)>> =
        std::collections::HashMap::new();
    let mut user_totals: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    for entry in &user_activities {
        per_user
            .entry(entry.user_id.clone())
            .or_default()
            .push((entry.activity.clone(), entry.total_minutes));
        *user_totals.entry(entry.user_id.clone()).or_insert(0) += entry.total_minutes;
    }

    let mut assignments: Vec<(String, String)> = Vec::new();

    let anchor_role_id: RoleId = match std::env::var("ANCHOR_ROLE_ID") {
        Ok(val) => match val.parse::<u64>() {
            Ok(id) => RoleId::new(id),
            Err(_) => {
                eprintln!("[roles] ANCHOR_ROLE_ID is not a valid u64");
                return Ok(0);
            }
        },
        Err(_) => {
            eprintln!("[roles] ANCHOR_ROLE_ID not set");
            return Ok(0);
        }
    };

    // Step 4: Classify and assign
    for (user_id, activities) in &per_user {
        let total = user_totals.get(user_id).copied().unwrap_or(0);
        if total == 0 {
            continue;
        }

        let (role_name, _word, tier) = match classifier.classify(activities, total) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("[roles] Classification failed for {}: {e}", user_id);
                continue;
            }
        };

        // Tier colours: cool → warm as hours increase
        let colour = match tier {
            1 => 0x95a5a6, // grey
            2 => 0x3498db, // blue
            3 => 0x2ecc71, // green
            4 => 0xf1c40f, // gold
            5 => 0xe67e22, // orange
            6 => 0xe74c3c, // red
            _ => 0x95a5a6,
        };

        // Create the role
        let role = create_role_above(http, guild_id, &role_name, colour, anchor_role_id).await;

        match role {
            Ok(role) => {
                let uid: u64 = match user_id.parse() {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let member_id = UserId::new(uid);

                // Assign the role
                if let Err(e) = http
                    .add_member_role(guild_id, member_id, role.id, Some("Weekly role assignment"))
                    .await
                {
                    eprintln!("[roles] Failed to assign role to {}: {e}", user_id);
                    continue;
                }

                // Set nickname with chevrons
                let member = guild_id.member(http, member_id).await.ok();
                let display_name = member
                    .as_ref()
                    .map(|m| {
                        if let Some(nick) = &m.nick {
                            nick.clone()
                        } else if let Some(global) = &m.user.global_name {
                            global.clone()
                        } else {
                            m.user.name.clone()
                        }
                    })
                    .unwrap_or_else(|| user_id.clone());

                let nickname = build_nickname(tier, &display_name);

                if let Err(e) = guild_id
                    .edit_member(http, member_id, EditMember::new().nickname(&nickname))
                    .await
                {
                    eprintln!("[roles] Failed to set nickname for {}: {e}", user_id);
                }

                println!("[roles] {} → {} (nick: {})", user_id, role_name, nickname);
                assignments.push((user_id.clone(), role_name));
                count += 1;
            }
            Err(e) => {
                eprintln!("[roles] Failed to create role '{}': {e}", role_name);
            }
        }
    }

    // Announce in summary channel
    if let Some(channel_id) = announce_channel {
        if !assignments.is_empty() {
            let mut lines: Vec<String> = Vec::new();
            for (user_id, role_name) in &assignments {
                lines.push(format!("<@{}> → **{}**", user_id, role_name));
            }
            let embed = CreateEmbed::new()
                .color(0xf1c40f)
                .title("🏆 Weekly Roles Assigned")
                .description(lines.join("\n"))
                .footer(CreateEmbedFooter::new(
                    db::now_ch().format("%d.%m.%Y %H:%M").to_string(),
                ));
            let _ = channel_id
                .send_message(http, CreateMessage::new().embed(embed))
                .await;
        }
    }

    Ok(count)
}
