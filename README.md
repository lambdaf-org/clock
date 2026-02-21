# ClockBot

Clock in/out tracker for Discord. SQLite or PostgreSQL backed, weekly resets, all-time leaderboard.

## Commands

```
/clock in <activity>      — start tracking
/clock out                — stop tracking, shows duration
/clock switch <activity>  — switch to new activity (auto clock-out/in)
/clock status             — your current session
/clock who                — who's working right now
/clock leaderboard        — weekly + all-time rankings
/clock stats              — activity breakdown per person
/clock recent             — your last 5 activities
/clock rename <old> > <new> — rename + merge activity
/clock help               — command list
```

### Aliases

Personal shortcuts to quickly clock into activities:

```
/clock alias <key> <activity>   — set personal alias
/clock aliases                  — list your aliases
/clock unalias <key>            — remove alias
```

### Admin Commands

Requires **Manage Guild** or **Administrator** permission:

```
/clock galias <key> <activity>  — set server-wide alias
/clock galiases                 — list global aliases
/clock gunalias <key>           — remove global alias
```

Alias resolution order: user alias → global alias → raw input.
The resolved activity name is stored in sessions.

## Setup

1. Create a Discord bot at https://discord.com/developers/applications
2. Enable **MESSAGE CONTENT** intent in Bot settings
3. Invite with scopes: `bot`, `applications.commands`  
   Permissions: Send Messages, Embed Links
4. Copy `.env.example` to `.env`, paste your token
5. `cargo run`

## Database

Set `DATABASE_URL` in `.env` to choose the backend.

**SQLite** (default):

```
DATABASE_URL=sqlite:///data/clock.db
```

**PostgreSQL**:

```
DATABASE_URL=postgres://user:password@host:5432/clockbot
```

If `DATABASE_URL` is not set, defaults to `sqlite:///data/clock.db`.

Tables are created automatically on first run for both backends.

## Environment Variables

```
DISCORD_TOKEN=       # required
DATABASE_URL=        # optional, defaults to sqlite
SUMMARY_CHANNEL=     # optional, channel ID for weekly summary posts
```

Weekly stats archive automatically every Monday at 00:00 Swiss time.
