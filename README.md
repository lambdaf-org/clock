# ClockBot

Clock in/out tracker for Discord. SQLite-backed, weekly resets, all-time leaderboard, activity breakdowns, and PNG line charts.

## Commands

```
/clock in <activity>                          — start tracking
/clock out                                    — stop tracking, shows duration
/clock status                                 — your current session
/clock who                                    — who's working right now
/clock leaderboard                            — weekly + all-time rankings (alias: /clock lb)
/clock stats                                  — weekly activity breakdown (top activities + per-person)
/clock rename <old> > <new>                   — rename and merge one of your activities
/clock chart [weeks] [totals|cumulative|both] — PNG line chart of top 5 weekly hours
/clock help                                   — command list
```

### `/clock chart` details

- `weeks` — number of weeks to plot (1–52, default `12`)
- mode:
  - `totals` *(default)* — hours worked per week, one line per user
  - `cumulative` — running total of hours over the range
  - `both` — totals and cumulative together
- Always shows the **top 5 users** by total hours in the selected window.

Examples:

```
/clock chart
/clock chart 8
/clock chart 26 cumulative
/clock chart 12 both
```

## Setup

1. Create a Discord bot at https://discord.com/developers/applications
2. Enable **MESSAGE CONTENT** intent in Bot settings
3. Invite with scopes: `bot`, `applications.commands`
   Permissions: Send Messages, Embed Links, Attach Files
4. Copy `.env.example` to `.env`, paste your token
5. `cargo run`

The bot creates `clock.db` in the working directory on first run.

Weekly stats archive automatically every Monday at 00:00 (Europe/Zurich).

## Deployment

The included `Dockerfile` produces a slim runtime image. Charts are rendered with
`plotters` using the pure-Rust `ab_glyph` backend and an embedded TTF font, so
**no system font packages are required** in the runtime container.
