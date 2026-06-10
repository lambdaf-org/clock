use crate::db::{ChartData, LeaderboardEntry, UserProfile};
use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};

/// Chart display mode.
pub enum ChartMode {
    Totals,
    Cumulative,
    Both,
}

impl ChartMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "cumulative" => ChartMode::Cumulative,
            "both" => ChartMode::Both,
            _ => ChartMode::Totals,
        }
    }
}

// Linear-style near-black theme.
const BG: RGBColor = RGBColor(0x02, 0x03, 0x05);
const FG: RGBColor = RGBColor(0xf4, 0xf4, 0xf5);
const MUTED: RGBColor = RGBColor(0x71, 0x71, 0x7a);
const GRID: RGBColor = RGBColor(0x18, 0x1a, 0x22);
const PANEL_BG: RGBColor = RGBColor(0x07, 0x08, 0x0c);
const PANEL_STROKE: RGBColor = RGBColor(0x1a, 0x1c, 0x24);
const WEEK_ACCENT: RGBColor = RGBColor(0xd8, 0xe2, 0xff);
const ALLTIME_ACCENT: RGBColor = RGBColor(0xa7, 0x8b, 0xfa);
const RANK_GOLD: RGBColor = RGBColor(0xf5, 0xc5, 0x42);
const RANK_SILVER: RGBColor = RGBColor(0xb2, 0xc8, 0xff);
const RANK_BRONZE: RGBColor = RGBColor(0xe0, 0x8a, 0x3e);
const LIVE_GREEN: RGBColor = RGBColor(0x2e, 0xcc, 0x71);
const MAX_LEADERBOARD_NAME_CHARS: usize = 18;
const MAX_CHART_LEGEND_CHARS: usize = 14;
const MAX_PROFILE_ACTIVITIES: usize = 6;
const MAX_PROFILE_ACTIVITY_CHARS: usize = 18;
const MAX_PROFILE_TITLE_CHARS: usize = 14;
const MAX_PROFILE_STATUS_CHARS: usize = 12;
const LEADERBOARD_CONTENT_BOTTOM_Y: i32 = 790;
const LEADERBOARD_MIN_ALLTIME_HEIGHT: i32 = 220;

// Muted analytics palette.
const PALETTE: [RGBColor; 5] = [
    RGBColor(0xd8, 0xe2, 0xff), // ice
    RGBColor(0xa7, 0x8b, 0xfa), // violet
    RGBColor(0x8b, 0x94, 0xa7), // slate
    RGBColor(0x67, 0xe8, 0xf9), // cyan
    RGBColor(0xe5, 0xe7, 0xeb), // zinc
];

/// Round `v` up to a "nice" number from a fixed sequence.
fn nice_ceiling(v: f64) -> f64 {
    const STEPS: &[f64] = &[
        5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 75.0, 100.0, 150.0, 200.0, 300.0, 500.0,
        1000.0, 1500.0, 2000.0, 3000.0, 5000.0,
    ];
    for &s in STEPS {
        if v <= s {
            return s;
        }
    }
    // fall back to next power-of-10 multiple
    let mag = 10f64.powf(v.log10().floor());
    (v / mag).ceil() * mag
}

/// Shorten week labels for display: `"KW14/2026"` → `"W14"`, others unchanged.
fn short_week_label(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("KW") {
        if let Some((num, _year)) = rest.split_once('/') {
            return format!("W{}", num);
        }
    }
    s.to_string()
}

/// Render a line chart for `data` according to `mode`.
/// Returns raw PNG bytes rendered entirely in memory.
pub fn render_chart(data: &ChartData, mode: ChartMode) -> anyhow::Result<Vec<u8>> {
    let n_weeks = data.week_labels.len();
    if n_weeks < 2 || data.users.is_empty() {
        anyhow::bail!("not enough data to render chart");
    }

    let (width, height): (u32, u32) = match mode {
        ChartMode::Both => (1200, 1360),
        _ => (1200, 760),
    };

    // Render to a raw RGB pixel buffer (3 bytes per pixel).
    let mut pixel_buf = vec![0u8; (width * height * 3) as usize];
    {
        let root = BitMapBackend::with_buffer(&mut pixel_buf, (width, height)).into_drawing_area();
        root.fill(&BG)
            .map_err(|e| anyhow::anyhow!("fill error: {:?}", e))?;

        match mode {
            ChartMode::Both => {
                let sub = root.split_evenly((2, 1));
                draw_panel(&sub[0], data, false, "Weekly hours · top 5")?;
                draw_panel(&sub[1], data, true, "Cumulative hours · top 5")?;
            }
            ChartMode::Cumulative => {
                draw_panel(&root, data, true, "Cumulative hours · top 5")?;
            }
            ChartMode::Totals => {
                draw_panel(&root, data, false, "Weekly hours · top 5")?;
            }
        }

        root.present()
            .map_err(|e| anyhow::anyhow!("present error: {:?}", e))?;
    }

    // Encode the raw RGB buffer to PNG in memory.
    let img = image::RgbImage::from_raw(width, height, pixel_buf)
        .ok_or_else(|| anyhow::anyhow!("failed to create RGB image from pixel buffer"))?;
    let mut png_bytes: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| anyhow::anyhow!("PNG encode error: {}", e))?;

    Ok(png_bytes)
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

fn truncate_name(name: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = name.chars().count();
    if count <= max_chars {
        return name.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", name.chars().take(keep).collect::<String>())
}

pub fn render_leaderboard_card(
    week_label: &str,
    weekly: &[LeaderboardEntry],
    alltime: &[LeaderboardEntry],
) -> anyhow::Result<Vec<u8>> {
    let width: u32 = 1120;
    let height: u32 = 860;
    let mut pixel_buf = vec![0u8; (width * height * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut pixel_buf, (width, height)).into_drawing_area();
        root.fill(&BG)
            .map_err(|e| anyhow::anyhow!("fill error: {:?}", e))?;

        root.draw(&Rectangle::new(
            [(24, 20), (1096, 806)],
            ShapeStyle::from(&PANEL_BG).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw card bg: {:?}", e))?;
        root.draw(&Rectangle::new(
            [(24, 20), (1096, 806)],
            ShapeStyle::from(&PANEL_STROKE).stroke_width(1),
        ))
        .map_err(|e| anyhow::anyhow!("draw card border: {:?}", e))?;

        root.draw(&Text::new(
            "Leaderboard",
            (56, 70),
            ("sans-serif", 42).into_font().color(&FG),
        ))
        .map_err(|e| anyhow::anyhow!("draw title: {:?}", e))?;

        root.draw(&Text::new(
            "Weekly and all-time tracked hours",
            (56, 104),
            ("sans-serif", 20).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw subtitle: {:?}", e))?;

        root.draw(&PathElement::new(
            vec![(56, 134), (1064, 134)],
            PANEL_STROKE.mix(0.85).stroke_width(1),
        ))
        .map_err(|e| anyhow::anyhow!("draw header divider: {:?}", e))?;

        let section_w = 1008;
        let left = 56;
        let week_top = 156;
        let weekly_rows = if weekly.is_empty() {
            1
        } else {
            weekly.len().min(3)
        } as i32;
        let week_h = 82 + (weekly_rows * 66);
        let alltime_top = week_top + week_h + 18;
        let alltime_h =
            (LEADERBOARD_CONTENT_BOTTOM_Y - alltime_top).max(LEADERBOARD_MIN_ALLTIME_HEIGHT);

        draw_leaderboard_section(
            &root,
            left,
            week_top,
            section_w,
            week_h,
            &format!("This week · {}", week_label),
            weekly,
            WEEK_ACCENT,
        )?;
        draw_leaderboard_section(
            &root,
            left,
            alltime_top,
            section_w,
            alltime_h,
            "All time",
            alltime,
            ALLTIME_ACCENT,
        )?;

        root.draw(&Text::new(
            "Resets every Monday 00:00 · Europe/Zurich",
            (56, 838),
            ("sans-serif", 18).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw footer: {:?}", e))?;

        root.present()
            .map_err(|e| anyhow::anyhow!("present error: {:?}", e))?;
    }

    let img = image::RgbImage::from_raw(width, height, pixel_buf)
        .ok_or_else(|| anyhow::anyhow!("failed to create RGB image from pixel buffer"))?;
    let mut png_bytes: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| anyhow::anyhow!("PNG encode error: {}", e))?;

    Ok(png_bytes)
}

fn draw_leaderboard_section<DB>(
    area: &DrawingArea<DB, plotters::coord::Shift>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    title: &str,
    entries: &[LeaderboardEntry],
    accent: RGBColor,
) -> anyhow::Result<()>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    area.draw(&Rectangle::new(
        [(x, y), (x + w, y + h)],
        ShapeStyle::from(&PANEL_BG.mix(0.8)).filled(),
    ))
    .map_err(|e| anyhow::anyhow!("draw section bg: {:?}", e))?;
    area.draw(&Rectangle::new(
        [(x, y), (x + w, y + h)],
        ShapeStyle::from(&accent.mix(0.42)).stroke_width(1),
    ))
    .map_err(|e| anyhow::anyhow!("draw section border: {:?}", e))?;

    area.draw(&Text::new(
        title,
        (x + 24, y + 44),
        ("sans-serif", 28).into_font().color(&FG),
    ))
    .map_err(|e| anyhow::anyhow!("draw section title: {:?}", e))?;

    let total_minutes: i64 = entries.iter().map(|e| e.total_minutes).sum();
    area.draw(&Text::new(
        format!("total · {}", format_duration(total_minutes)),
        (x + w - 24, y + 44),
        ("sans-serif", 19)
            .into_font()
            .color(&MUTED)
            .pos(Pos::new(HPos::Right, VPos::Center)),
    ))
    .map_err(|e| anyhow::anyhow!("draw section total: {:?}", e))?;

    area.draw(&PathElement::new(
        vec![(x + 24, y + 62), (x + w - 24, y + 62)],
        PANEL_STROKE.mix(0.75).stroke_width(1),
    ))
    .map_err(|e| anyhow::anyhow!("draw section divider: {:?}", e))?;

    if entries.is_empty() {
        area.draw(&Text::new(
            "No data yet.",
            (x + 24, y + 126),
            ("sans-serif", 30).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw empty text: {:?}", e))?;
        return Ok(());
    }

    let rank_x = x + 24;
    let name_x = x + 120;
    let dur_x = x + w - 24;
    let row_start_y = y + 74;
    const HEADER_HEIGHT: i32 = 82;
    const ROW_HEIGHT: i32 = 62;
    const ROW_TEXT_Y_OFFSET: i32 = 30;
    const BAR_RIGHT_MARGIN: i32 = 210;
    const BAR_MAX_WIDTH: i32 = 180;
    const BAR_HEIGHT: i32 = 16;
    const BAR_MIN_FILL_WIDTH: i32 = 2;
    let max_entries = ((h - HEADER_HEIGHT) / ROW_HEIGHT).max(1) as usize;
    let visible = entries.iter().take(max_entries).collect::<Vec<_>>();
    let max_visible_minutes = visible
        .iter()
        .map(|entry| entry.total_minutes)
        .max()
        .unwrap_or(1)
        .max(1);
    let bar_right = x + w - BAR_RIGHT_MARGIN;

    for (idx, entry) in visible.iter().enumerate() {
        let row_y = row_start_y + (idx as i32 * ROW_HEIGHT);
        let rank_color = rank_color(idx);
        let rank_label = rank_label(idx);

        if idx > 0 {
            area.draw(&PathElement::new(
                vec![(x + 24, row_y - 12), (x + w - 24, row_y - 12)],
                PANEL_STROKE.mix(0.45).stroke_width(1),
            ))
            .map_err(|e| anyhow::anyhow!("draw row divider: {:?}", e))?;
        }

        area.draw(&Text::new(
            rank_label,
            (rank_x, row_y + ROW_TEXT_Y_OFFSET),
            ("sans-serif", 32).into_font().color(&rank_color),
        ))
        .map_err(|e| anyhow::anyhow!("draw rank: {:?}", e))?;

        area.draw(&Text::new(
            truncate_name(&entry.username, MAX_LEADERBOARD_NAME_CHARS),
            (name_x, row_y + ROW_TEXT_Y_OFFSET),
            ("sans-serif", 30).into_font().color(&FG),
        ))
        .map_err(|e| anyhow::anyhow!("draw name: {:?}", e))?;

        let bar_fill_width = ((entry.total_minutes * BAR_MAX_WIDTH as i64) / max_visible_minutes)
            .max(BAR_MIN_FILL_WIDTH as i64) as i32;
        let bar_top = row_y + ROW_TEXT_Y_OFFSET - (BAR_HEIGHT / 2);
        area.draw(&Rectangle::new(
            [
                (bar_right - BAR_MAX_WIDTH, bar_top),
                (bar_right, bar_top + BAR_HEIGHT),
            ],
            ShapeStyle::from(&PANEL_STROKE.mix(0.9)).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw row bar bg: {:?}", e))?;
        area.draw(&Rectangle::new(
            [
                (bar_right - BAR_MAX_WIDTH, bar_top),
                (bar_right - BAR_MAX_WIDTH + bar_fill_width, bar_top + BAR_HEIGHT),
            ],
            ShapeStyle::from(&accent.mix(0.75)).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw row bar fill: {:?}", e))?;

        area.draw(&Text::new(
            format_duration(entry.total_minutes),
            (dur_x, row_y + ROW_TEXT_Y_OFFSET),
            ("sans-serif", 34)
                .into_font()
                .color(&FG)
                .pos(Pos::new(HPos::Right, VPos::Center)),
        ))
        .map_err(|e| anyhow::anyhow!("draw duration: {:?}", e))?;
    }

    Ok(())
}

/// Render a per-user activity profile card.
/// Layout: header with live status, four stat tiles, an all-time activity
/// breakdown, and a weekly trend strip. Image height adapts to the number
/// of activity rows.
pub fn render_user_card(profile: &UserProfile) -> anyhow::Result<Vec<u8>> {
    let width: u32 = 1120;
    let left: i32 = 56;
    let section_w: i32 = 1008;

    let tiles_top: i32 = 160;
    let tiles_h: i32 = 96;

    let shown = profile.top_activities.len().min(MAX_PROFILE_ACTIVITIES);
    let overflow_count = profile.top_activities.len().saturating_sub(shown);
    let act_top = tiles_top + tiles_h + 22;
    let act_rows = shown.max(1) as i32; // one row minimum for the empty message
    let act_h = 70 + act_rows * 54 + if overflow_count > 0 { 40 } else { 0 } + 14;

    let trend_top = act_top + act_h + 22;
    let trend_h: i32 = 208;

    let card_bottom = trend_top + trend_h + 26;
    let footer_y = card_bottom + 34;
    let height = (footer_y + 26) as u32;

    let mut pixel_buf = vec![0u8; (width * height * 3) as usize];
    {
        let root = BitMapBackend::with_buffer(&mut pixel_buf, (width, height)).into_drawing_area();
        root.fill(&BG)
            .map_err(|e| anyhow::anyhow!("fill error: {:?}", e))?;

        root.draw(&Rectangle::new(
            [(24, 20), (1096, card_bottom)],
            ShapeStyle::from(&PANEL_BG).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw card bg: {:?}", e))?;
        root.draw(&Rectangle::new(
            [(24, 20), (1096, card_bottom)],
            ShapeStyle::from(&PANEL_STROKE).stroke_width(1),
        ))
        .map_err(|e| anyhow::anyhow!("draw card border: {:?}", e))?;

        // ── Header ────────────────────────────────────────────
        root.draw(&Text::new(
            truncate_name(&profile.username, MAX_PROFILE_TITLE_CHARS),
            (left, 56),
            ("sans-serif", 42).into_font().color(&FG),
        ))
        .map_err(|e| anyhow::anyhow!("draw title: {:?}", e))?;

        root.draw(&Text::new(
            "activity profile · all time",
            (left, 104),
            ("sans-serif", 20).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw subtitle: {:?}", e))?;

        let (status_text, status_color) = match &profile.active_session {
            Some((activity, elapsed)) => (
                format!(
                    "● {} · {}",
                    truncate_name(activity, MAX_PROFILE_STATUS_CHARS),
                    format_duration(*elapsed)
                ),
                LIVE_GREEN,
            ),
            None => ("○ not clocked in".to_string(), MUTED),
        };
        root.draw(&Text::new(
            status_text,
            (left + section_w, 70),
            ("sans-serif", 22)
                .into_font()
                .color(&status_color)
                .pos(Pos::new(HPos::Right, VPos::Center)),
        ))
        .map_err(|e| anyhow::anyhow!("draw status: {:?}", e))?;

        root.draw(&PathElement::new(
            vec![(left, 134), (left + section_w, 134)],
            PANEL_STROKE.mix(0.85).stroke_width(1),
        ))
        .map_err(|e| anyhow::anyhow!("draw header divider: {:?}", e))?;

        // ── Stat tiles ────────────────────────────────────────
        let best_label = match &profile.best_week {
            Some((label, _)) => format!("best week · {}", short_week_label(label)),
            None => "best week".to_string(),
        };
        let best_value = match &profile.best_week {
            Some((_, minutes)) => format_duration(*minutes),
            None => "—".to_string(),
        };
        let tiles: [(String, String); 4] = [
            ("total".to_string(), format_duration(profile.total_minutes)),
            (
                "this week".to_string(),
                format_duration(profile.current_week_minutes),
            ),
            (best_label, best_value),
            (
                "active weeks".to_string(),
                format!("{}", profile.active_weeks),
            ),
        ];
        const TILE_W: i32 = 240;
        const TILE_GAP: i32 = 16; // 4 * 240 + 3 * 16 == section_w
        for (i, (label, value)) in tiles.iter().enumerate() {
            let tx = left + i as i32 * (TILE_W + TILE_GAP);
            root.draw(&Rectangle::new(
                [(tx, tiles_top), (tx + TILE_W, tiles_top + tiles_h)],
                ShapeStyle::from(&PANEL_BG.mix(0.8)).filled(),
            ))
            .map_err(|e| anyhow::anyhow!("draw tile bg: {:?}", e))?;
            root.draw(&Rectangle::new(
                [(tx, tiles_top), (tx + TILE_W, tiles_top + tiles_h)],
                ShapeStyle::from(&PANEL_STROKE).stroke_width(1),
            ))
            .map_err(|e| anyhow::anyhow!("draw tile border: {:?}", e))?;
            root.draw(&Text::new(
                label.clone(),
                (tx + 18, tiles_top + 18),
                ("sans-serif", 17).into_font().color(&MUTED),
            ))
            .map_err(|e| anyhow::anyhow!("draw tile label: {:?}", e))?;
            root.draw(&Text::new(
                value.clone(),
                (tx + 18, tiles_top + 46),
                ("sans-serif", 30).into_font().color(&FG),
            ))
            .map_err(|e| anyhow::anyhow!("draw tile value: {:?}", e))?;
        }

        draw_profile_activities(
            &root,
            left,
            act_top,
            section_w,
            act_h,
            profile,
            shown,
            overflow_count,
        )?;
        draw_profile_trend(&root, left, trend_top, section_w, trend_h, profile)?;

        root.draw(&Text::new(
            "Tracked with /clock · Europe/Zurich",
            (left, footer_y),
            ("sans-serif", 18).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw footer: {:?}", e))?;

        root.present()
            .map_err(|e| anyhow::anyhow!("present error: {:?}", e))?;
    }

    let img = image::RgbImage::from_raw(width, height, pixel_buf)
        .ok_or_else(|| anyhow::anyhow!("failed to create RGB image from pixel buffer"))?;
    let mut png_bytes: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| anyhow::anyhow!("PNG encode error: {}", e))?;

    Ok(png_bytes)
}

#[allow(clippy::too_many_arguments)]
fn draw_profile_activities<DB>(
    area: &DrawingArea<DB, plotters::coord::Shift>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    profile: &UserProfile,
    shown: usize,
    overflow_count: usize,
) -> anyhow::Result<()>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    area.draw(&Rectangle::new(
        [(x, y), (x + w, y + h)],
        ShapeStyle::from(&PANEL_BG.mix(0.8)).filled(),
    ))
    .map_err(|e| anyhow::anyhow!("draw activities bg: {:?}", e))?;
    area.draw(&Rectangle::new(
        [(x, y), (x + w, y + h)],
        ShapeStyle::from(&ALLTIME_ACCENT.mix(0.42)).stroke_width(1),
    ))
    .map_err(|e| anyhow::anyhow!("draw activities border: {:?}", e))?;

    area.draw(&Text::new(
        "what they do",
        (x + 24, y + 26),
        ("sans-serif", 26).into_font().color(&FG),
    ))
    .map_err(|e| anyhow::anyhow!("draw activities title: {:?}", e))?;

    area.draw(&Text::new(
        format!("{} activities", profile.top_activities.len()),
        (x + w - 24, y + 40),
        ("sans-serif", 19)
            .into_font()
            .color(&MUTED)
            .pos(Pos::new(HPos::Right, VPos::Center)),
    ))
    .map_err(|e| anyhow::anyhow!("draw activities count: {:?}", e))?;

    area.draw(&PathElement::new(
        vec![(x + 24, y + 58), (x + w - 24, y + 58)],
        PANEL_STROKE.mix(0.75).stroke_width(1),
    ))
    .map_err(|e| anyhow::anyhow!("draw activities divider: {:?}", e))?;

    if shown == 0 {
        area.draw(&Text::new(
            "No completed sessions yet.",
            (x + 24, y + 84),
            ("sans-serif", 24).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw activities empty: {:?}", e))?;
        return Ok(());
    }

    const ROW_HEIGHT: i32 = 54;
    const BAR_LEFT_OFFSET: i32 = 460;
    const BAR_MAX_WIDTH: i64 = 300;
    const BAR_HEIGHT: i32 = 14;
    let max_minutes = profile
        .top_activities
        .first()
        .map(|(_, m)| *m)
        .unwrap_or(1)
        .max(1);
    let total = profile.total_minutes.max(1);

    for (idx, (activity, minutes)) in profile.top_activities.iter().take(shown).enumerate() {
        let row_y = y + 70 + idx as i32 * ROW_HEIGHT;

        if idx > 0 {
            area.draw(&PathElement::new(
                vec![(x + 24, row_y - 6), (x + w - 24, row_y - 6)],
                PANEL_STROKE.mix(0.45).stroke_width(1),
            ))
            .map_err(|e| anyhow::anyhow!("draw activity row divider: {:?}", e))?;
        }

        area.draw(&Text::new(
            truncate_name(activity, MAX_PROFILE_ACTIVITY_CHARS),
            (x + 24, row_y + 12),
            ("sans-serif", 25).into_font().color(&FG),
        ))
        .map_err(|e| anyhow::anyhow!("draw activity name: {:?}", e))?;

        let bar_left = x + BAR_LEFT_OFFSET;
        let bar_top = row_y + 19;
        let fill = ((minutes * BAR_MAX_WIDTH) / max_minutes).max(2) as i32;
        area.draw(&Rectangle::new(
            [
                (bar_left, bar_top),
                (bar_left + BAR_MAX_WIDTH as i32, bar_top + BAR_HEIGHT),
            ],
            ShapeStyle::from(&PANEL_STROKE.mix(0.9)).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw activity bar bg: {:?}", e))?;
        area.draw(&Rectangle::new(
            [(bar_left, bar_top), (bar_left + fill, bar_top + BAR_HEIGHT)],
            ShapeStyle::from(&ALLTIME_ACCENT.mix(0.8)).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw activity bar fill: {:?}", e))?;

        let pct = (minutes * 100 + total / 2) / total;
        area.draw(&Text::new(
            format!("{} · {}%", format_duration(*minutes), pct),
            (x + w - 24, row_y + 26),
            ("sans-serif", 24)
                .into_font()
                .color(&FG)
                .pos(Pos::new(HPos::Right, VPos::Center)),
        ))
        .map_err(|e| anyhow::anyhow!("draw activity value: {:?}", e))?;
    }

    if overflow_count > 0 {
        let rest_minutes: i64 = profile
            .top_activities
            .iter()
            .skip(shown)
            .map(|(_, m)| *m)
            .sum();
        let row_y = y + 70 + shown as i32 * ROW_HEIGHT;
        area.draw(&Text::new(
            format!(
                "+ {} more · {}",
                overflow_count,
                format_duration(rest_minutes)
            ),
            (x + 24, row_y + 8),
            ("sans-serif", 20).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw activity overflow: {:?}", e))?;
    }

    Ok(())
}

fn draw_profile_trend<DB>(
    area: &DrawingArea<DB, plotters::coord::Shift>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    profile: &UserProfile,
) -> anyhow::Result<()>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    area.draw(&Rectangle::new(
        [(x, y), (x + w, y + h)],
        ShapeStyle::from(&PANEL_BG.mix(0.8)).filled(),
    ))
    .map_err(|e| anyhow::anyhow!("draw trend bg: {:?}", e))?;
    area.draw(&Rectangle::new(
        [(x, y), (x + w, y + h)],
        ShapeStyle::from(&WEEK_ACCENT.mix(0.42)).stroke_width(1),
    ))
    .map_err(|e| anyhow::anyhow!("draw trend border: {:?}", e))?;

    let n = profile.week_labels.len().max(1);
    area.draw(&Text::new(
        format!("last {} weeks", n),
        (x + 24, y + 26),
        ("sans-serif", 26).into_font().color(&FG),
    ))
    .map_err(|e| anyhow::anyhow!("draw trend title: {:?}", e))?;

    let max_minutes = profile.weekly_minutes.iter().copied().max().unwrap_or(0);
    let right_text = if max_minutes > 0 {
        format!("peak · {}", format_duration(max_minutes))
    } else {
        "no data in window".to_string()
    };
    area.draw(&Text::new(
        right_text,
        (x + w - 24, y + 40),
        ("sans-serif", 19)
            .into_font()
            .color(&MUTED)
            .pos(Pos::new(HPos::Right, VPos::Center)),
    ))
    .map_err(|e| anyhow::anyhow!("draw trend peak: {:?}", e))?;

    area.draw(&PathElement::new(
        vec![(x + 24, y + 58), (x + w - 24, y + 58)],
        PANEL_STROKE.mix(0.75).stroke_width(1),
    ))
    .map_err(|e| anyhow::anyhow!("draw trend divider: {:?}", e))?;

    let inner_left = x + 24;
    let inner_w = w - 48;
    let bars_bottom = y + 162;
    // Short enough that the peak label above the tallest bar clears the
    // section divider at y + 58.
    let bars_max_h: i32 = 72;
    let slot = inner_w / n as i32;
    let bar_w = (slot * 3 / 5).max(2);
    let scale_max = max_minutes.max(1);

    let mut peak_center: Option<(i32, i32)> = None;
    for (i, minutes) in profile.weekly_minutes.iter().enumerate() {
        let bx = inner_left + i as i32 * slot + (slot - bar_w) / 2;
        let is_current = i == n - 1;
        if *minutes > 0 {
            let bar_h = ((*minutes * bars_max_h as i64) / scale_max).max(3) as i32;
            let color = if is_current {
                WEEK_ACCENT.mix(1.0)
            } else {
                WEEK_ACCENT.mix(0.55)
            };
            area.draw(&Rectangle::new(
                [(bx, bars_bottom - bar_h), (bx + bar_w, bars_bottom)],
                ShapeStyle::from(&color).filled(),
            ))
            .map_err(|e| anyhow::anyhow!("draw trend bar: {:?}", e))?;
            if *minutes == max_minutes && peak_center.is_none() {
                peak_center = Some((bx + bar_w / 2, bars_bottom - bar_h));
            }
        } else {
            area.draw(&Rectangle::new(
                [(bx, bars_bottom - 2), (bx + bar_w, bars_bottom)],
                ShapeStyle::from(&MUTED.mix(0.3)).filled(),
            ))
            .map_err(|e| anyhow::anyhow!("draw trend stub: {:?}", e))?;
        }
    }

    area.draw(&PathElement::new(
        vec![(inner_left, bars_bottom + 1), (x + w - 24, bars_bottom + 1)],
        PANEL_STROKE.mix(0.9).stroke_width(1),
    ))
    .map_err(|e| anyhow::anyhow!("draw trend baseline: {:?}", e))?;

    if let Some((cx, top)) = peak_center {
        area.draw(&Text::new(
            format_duration(max_minutes),
            (cx, top - 16),
            ("sans-serif", 16)
                .into_font()
                .color(&MUTED)
                .pos(Pos::new(HPos::Center, VPos::Center)),
        ))
        .map_err(|e| anyhow::anyhow!("draw trend peak label: {:?}", e))?;
    }

    let step = n.div_ceil(6).max(1);
    for (i, label) in profile.week_labels.iter().enumerate() {
        if i % step != 0 && i != n - 1 {
            continue;
        }
        let cx = inner_left + i as i32 * slot + slot / 2;
        area.draw(&Text::new(
            short_week_label(label),
            (cx, bars_bottom + 16),
            ("sans-serif", 16)
                .into_font()
                .color(&MUTED)
                .pos(Pos::new(HPos::Center, VPos::Center)),
        ))
        .map_err(|e| anyhow::anyhow!("draw trend label: {:?}", e))?;
    }

    Ok(())
}

fn rank_label(idx: usize) -> String {
    match idx {
        0 => "1st".to_string(),
        1 => "2nd".to_string(),
        2 => "3rd".to_string(),
        _ => format!("#{}", idx + 1),
    }
}

fn rank_color(idx: usize) -> RGBColor {
    match idx {
        0 => RANK_GOLD,
        1 => RANK_SILVER,
        2 => RANK_BRONZE,
        _ => MUTED,
    }
}

fn truncate_legend_label(name: &str) -> String {
    truncate_name(name, MAX_CHART_LEGEND_CHARS)
}

fn draw_panel<DB>(
    area: &DrawingArea<DB, plotters::coord::Shift>,
    data: &ChartData,
    cumulative: bool,
    title: &str,
) -> anyhow::Result<()>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let n_weeks = data.week_labels.len();
    let (area_w, area_h) = area.dim_in_pixel();
    let card_left = 22;
    let card_top = 18;
    let card_right = area_w as i32 - 22;
    let card_bottom = area_h as i32 - 20;

    area.draw(&Rectangle::new(
        [(card_left, card_top), (card_right, card_bottom)],
        ShapeStyle::from(&PANEL_BG).filled(),
    ))
    .map_err(|e| anyhow::anyhow!("draw chart card bg: {:?}", e))?;
    area.draw(&Rectangle::new(
        [(card_left, card_top), (card_right, card_bottom)],
        ShapeStyle::from(&PANEL_STROKE).stroke_width(1),
    ))
    .map_err(|e| anyhow::anyhow!("draw chart card border: {:?}", e))?;

    // Compute the y-axis maximum.
    let raw_max: f64 = if cumulative {
        data.users
            .iter()
            .map(|u| {
                u.minutes_per_week
                    .iter()
                    .map(|&m| m as f64 / 60.0)
                    .sum::<f64>()
            })
            .fold(0.0_f64, f64::max)
    } else {
        data.users
            .iter()
            .flat_map(|u| u.minutes_per_week.iter())
            .map(|&m| m as f64 / 60.0)
            .fold(0.0_f64, f64::max)
    };
    let y_max = nice_ceiling(raw_max * 1.05).max(1.0);

    let x_max = (n_weeks - 1) as i32;

    // Show at most 8 X tick labels.
    let label_count = n_weeks.min(6);

    let mut chart = ChartBuilder::on(area)
        .caption(title, ("sans-serif", 34).into_font().color(&FG))
        .margin(44)
        .x_label_area_size(72)
        .y_label_area_size(86)
        .build_cartesian_2d(0i32..x_max, 0.0f64..y_max)
        .map_err(|e| anyhow::anyhow!("build chart: {:?}", e))?;

    let week_labels = &data.week_labels;
    chart
        .configure_mesh()
        .disable_x_mesh()
        .bold_line_style(GRID.mix(0.55))
        .light_line_style(GRID.mix(0.35))
        .axis_style(MUTED.mix(0.0)) // fully transparent — no visible axis line
        .x_labels(label_count)
        .y_labels(6)
        .x_label_style(("sans-serif", 20).into_font().color(&MUTED))
        .x_label_formatter(&|x| {
            week_labels
                .get(*x as usize)
                .map(|s| short_week_label(s))
                .unwrap_or_default()
        })
        .y_label_style(("sans-serif", 20).into_font().color(&MUTED))
        .y_label_formatter(&|y| format!("{:.0}h", y))
        .draw()
        .map_err(|e| anyhow::anyhow!("configure mesh: {:?}", e))?;

    for (i, user) in data.users.iter().enumerate() {
        let color = PALETTE[i % PALETTE.len()];

        // Build (x, y) data points.
        let points: Vec<(i32, f64)> = if cumulative {
            let mut cumsum = 0.0f64;
            user.minutes_per_week
                .iter()
                .enumerate()
                .map(|(x, &m)| {
                    cumsum += m as f64 / 60.0;
                    (x as i32, cumsum)
                })
                .collect()
        } else {
            user.minutes_per_week
                .iter()
                .enumerate()
                .map(|(x, &m)| (x as i32, m as f64 / 60.0))
                .collect()
        };

        // Main line — restrained, no glow.
        let username = truncate_legend_label(&user.username);
        chart
            .draw_series(LineSeries::new(
                points.clone(),
                color.mix(0.82).stroke_width(4),
            ))
            .map_err(|e| anyhow::anyhow!("draw line: {:?}", e))?
            .label(username)
            .legend(move |(lx, ly)| {
                PathElement::new(
                    vec![(lx, ly), (lx + 34, ly)],
                    color.mix(0.82).stroke_width(4),
                )
            });

        if let Some(&last_pt) = points.last() {
            chart
                .draw_series(std::iter::once(Circle::new(
                    last_pt,
                    6,
                    color.mix(0.82).filled(),
                )))
                .map_err(|e| anyhow::anyhow!("draw endpoint: {:?}", e))?;
        }
    }

    chart
        .configure_series_labels()
        .background_style(PANEL_BG.mix(0.94).filled())
        .border_style(PANEL_STROKE.mix(0.6))
        .label_font(("sans-serif", 18).into_font().color(&FG))
        .position(SeriesLabelPosition::UpperLeft)
        .draw()
        .map_err(|e| anyhow::anyhow!("draw legend: {:?}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ChartData, LeaderboardEntry, UserWeeklyData};

    fn make_data() -> ChartData {
        ChartData {
            week_labels: vec![
                "KW14/2026".to_string(),
                "KW15/2026".to_string(),
                "KW16/2026".to_string(),
                "KW17/2026".to_string(),
            ],
            users: vec![
                UserWeeklyData {
                    username: "Alice".to_string(),
                    minutes_per_week: vec![120, 90, 180, 60],
                },
                UserWeeklyData {
                    username: "Bob".to_string(),
                    minutes_per_week: vec![60, 150, 30, 90],
                },
            ],
        }
    }

    fn register_test_font() {
        static FONT: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
        use plotters::style::{register_font, FontStyle};
        for style in [
            FontStyle::Normal,
            FontStyle::Bold,
            FontStyle::Italic,
            FontStyle::Oblique,
        ] {
            // "already registered" errors are fine in parallel tests; ignore them.
            let _ = register_font("sans-serif", style, FONT);
        }
    }

    fn sample_leaderboard() -> Vec<LeaderboardEntry> {
        vec![
            LeaderboardEntry {
                username: "Bimodality".to_string(),
                total_minutes: 3980,
            },
            LeaderboardEntry {
                username: "Thirsty".to_string(),
                total_minutes: 517,
            },
            LeaderboardEntry {
                username: "Sora".to_string(),
                total_minutes: 412,
            },
        ]
    }

    #[test]
    fn test_nice_ceiling() {
        assert!(nice_ceiling(0.0) >= 1.0);
        assert_eq!(nice_ceiling(3.2), 5.0);
        assert_eq!(nice_ceiling(11.0), 15.0);
        assert_eq!(nice_ceiling(99.0), 100.0);
    }

    #[test]
    fn test_short_week_label() {
        assert_eq!(short_week_label("KW14/2026"), "W14");
        assert_eq!(short_week_label("foo"), "foo");
    }

    #[test]
    fn test_truncate_name() {
        assert_eq!(truncate_name("alice", 0), "");
        assert_eq!(truncate_name("alice", 10), "alice");
        assert_eq!(truncate_name("123456", 6), "123456");
        assert_eq!(truncate_name("1234567", 6), "12345…");
        assert_eq!(truncate_name("überlangername", 5), "über…");
    }

    #[test]
    fn test_rank_labels_and_legend_truncation() {
        assert_eq!(rank_label(0), "1st");
        assert_eq!(rank_label(1), "2nd");
        assert_eq!(rank_label(2), "3rd");
        assert_eq!(rank_label(3), "#4");

        assert_eq!(truncate_legend_label("Alice"), "Alice");
        assert_eq!(
            truncate_legend_label("averyveryverylongusername"),
            "averyveryvery…"
        );
    }

    #[test]
    fn test_render_totals_produces_png() {
        register_test_font();
        let data = make_data();
        let bytes = render_chart(&data, ChartMode::Totals).expect("render failed");
        // PNG magic bytes: 0x89 P N G \r \n 0x1a \n
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "output is not a PNG"
        );
        assert!(bytes.len() > 1000, "PNG seems too small");
    }

    #[test]
    fn test_render_cumulative_produces_png() {
        register_test_font();
        let data = make_data();
        let bytes = render_chart(&data, ChartMode::Cumulative).expect("render failed");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn test_render_both_produces_larger_png() {
        register_test_font();
        let data = make_data();
        let totals = render_chart(&data, ChartMode::Totals).expect("render totals failed");
        let both = render_chart(&data, ChartMode::Both).expect("render both failed");
        // "both" is 1200×900 vs 1200×600 so should produce more pixel data.
        assert!(both.len() > totals.len());
    }

    #[test]
    fn test_render_insufficient_data_errors() {
        let data = ChartData {
            week_labels: vec!["KW14/2026".to_string()], // only 1 week
            users: vec![UserWeeklyData {
                username: "Alice".to_string(),
                minutes_per_week: vec![60],
            }],
        };
        assert!(render_chart(&data, ChartMode::Totals).is_err());
    }

    #[test]
    fn test_render_no_users_errors() {
        let data = ChartData {
            week_labels: vec!["KW14/2026".to_string(), "KW15/2026".to_string()],
            users: vec![],
        };
        assert!(render_chart(&data, ChartMode::Totals).is_err());
    }

    fn sample_profile() -> crate::db::UserProfile {
        crate::db::UserProfile {
            username: "Alice".to_string(),
            total_minutes: 1234,
            current_week_minutes: 230,
            active_weeks: 7,
            best_week: Some(("KW15/2026".to_string(), 480)),
            top_activities: vec![
                ("writing".to_string(), 400),
                ("reading".to_string(), 300),
                ("thesis".to_string(), 200),
                ("review".to_string(), 120),
                ("meeting".to_string(), 90),
                ("planning".to_string(), 60),
                ("email".to_string(), 40),
                ("misc".to_string(), 24),
            ],
            week_labels: (14..=25).map(|w| format!("KW{:02}/2026", w)).collect(),
            weekly_minutes: vec![0, 60, 480, 120, 0, 90, 200, 44, 0, 0, 10, 230],
            active_session: Some(("writing".to_string(), 42)),
        }
    }

    #[test]
    fn test_render_user_card_produces_png() {
        register_test_font();
        let profile = sample_profile();
        let bytes = render_user_card(&profile).expect("render failed");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(bytes.len() > 10_000, "user card PNG seems too small");
    }

    #[test]
    fn test_render_user_card_empty_profile() {
        register_test_font();
        let profile = crate::db::UserProfile {
            username: "Alice".to_string(),
            total_minutes: 0,
            current_week_minutes: 0,
            active_weeks: 0,
            best_week: None,
            top_activities: vec![],
            week_labels: (14..=25).map(|w| format!("KW{:02}/2026", w)).collect(),
            weekly_minutes: vec![0; 12],
            active_session: None,
        };
        let bytes = render_user_card(&profile).expect("render failed");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn test_render_leaderboard_card_produces_png() {
        register_test_font();
        let weekly = sample_leaderboard();
        let alltime = sample_leaderboard();
        let bytes = render_leaderboard_card("KW21/2026", &weekly, &alltime).expect("render failed");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(bytes.len() > 10_000, "leaderboard card PNG seems too small");
    }
}
