# ClockBot

Clock in/out tracker for Discord. SQLite or PostgreSQL backed, weekly resets, all-time leaderboard. Automatic role classification and rank nicknames.

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

## Roles

Every Monday at reset, ClockBot classifies each user's weekly activity using sentence embeddings (all-MiniLM-L6-v2) and assigns a Discord role based on work style and total hours.

### Styles

Activities are matched against 7 style archetypes via cosine similarity:

| Style | Signal |
|-------|--------|
| Architect | infrastructure, tooling, bots, automation |
| Visionary | design, products, landing pages, prototypes |
| Executor | physical labor, manual work |
| Analyst | research, papers, experiments, coursework |
| Ghost | maintenance, cleanup, background fixes |
| Strategist | planning, coordination, roadmaps |
| Maverick | side projects, experiments, hobby builds |

### Tiers

Total weekly minutes determine tier:

| Tier | Minutes | Colour | Font |
|------|---------|--------|------|
| 1 | 0+ | Grey | Plain |
| 2 | 1200+ | Blue | Plain |
| 3 | 2400+ | Green | 𝐼𝑡𝑎𝑙𝑖𝑐 |
| 4 | 3600+ | Gold | **𝐁𝐨𝐥𝐝 𝐬𝐞𝐫𝐢𝐟** |
| 5 | 4500+ | Orange | **𝗕𝗼𝗹𝗱 𝘀𝗮𝗻𝘀** |
| 6 | 5400+ | Red | 𝔉𝔯𝔞𝔨𝔱𝔲𝔯 |

### Role Format

Tier 1 roles are plain text. Tier 2+ get bracketed chevrons:

```
Spark
〔⟫〕Workhorse
〔⟫⟫〕𝐴𝑝𝑝𝑎𝑟𝑖𝑡𝑖𝑜𝑛
〔⟫⟫⟫〕𝐅𝐨𝐫𝐭𝐫𝐞𝐬𝐬
〔⟫⟫⟫⟫〕𝗖𝗵𝗲𝘀𝘀𝗺𝗮𝘀𝘁𝗲𝗿
〔⟫⟫⟫⟫⟫〕𝔑𝔬𝔱𝔥𝔦𝔫𝔤
```

### Nicknames

Server nicknames reflect tier as chevron count:

```
Alex
⟫ | Ben
⟫⟫ | Chris
⟫⟫⟫ | Dave
⟫⟫⟫⟫ | Eve
⟫⟫⟫⟫⟫ | Frank
```

Nicknames reset every Monday before reassignment.

## Setup

1. Create a Discord bot at https://discord.com/developers/applications
2. Enable **MESSAGE CONTENT** intent in Bot settings
3. Invite with scopes: `bot`, `applications.commands`
   Permissions: Send Messages, Embed Links, Manage Roles, Manage Nicknames
4. Copy `.env.example` to `.env`, paste your token
5. Create an anchor role in your server (roles are positioned above it)
6. `cargo run`

The embedding model (all-MiniLM-L6-v2) downloads automatically on first run and is cached for subsequent starts.

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
GUILD_ID=            # required for role assignment
ANCHOR_ROLE_ID=      # required for role positioning
```

Weekly stats archive and role reassignment happen every Monday at 00:00 Swiss time.
