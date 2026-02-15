# ClockBot

Clock in/out tracker for Discord. SQLite-backed, weekly resets, all-time leaderboard.

## Commands

```
/clock in <activity>   — start tracking
/clock out             — stop tracking, shows duration
/clock status          — your current session
/clock who             — who's working right now
/clock leaderboard     — weekly + all-time rankings
/clock help            — command list
```

## Setup

1. Create a Discord bot at https://discord.com/developers/applications
2. Enable **MESSAGE CONTENT** intent in Bot settings
3. Invite with scopes: `bot`, `applications.commands`  
   Permissions: Send Messages, Embed Links
4. Copy `.env.example` to `.env`, paste your token
5. `cargo run`

The bot creates `clock.db` in the working directory on first run.

Weekly stats archive automatically every Monday at 00:00 UTC.
