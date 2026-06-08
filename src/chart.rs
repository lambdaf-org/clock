use crate::db::{ChartData, LeaderboardEntry};
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
const MAX_LEADERBOARD_NAME_CHARS: usize = 18;
const MAX_CHART_LEGEND_CHARS: usize = 14;
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
    let max_entries = ((h - HEADER_HEIGHT) / ROW_HEIGHT).max(1) as usize;
    let visible = entries.iter().take(max_entries).collect::<Vec<_>>();
    let max_visible_minutes = visible
        .iter()
        .map(|entry| entry.total_minutes)
        .max()
        .unwrap_or(1)
        .max(1);
    let bar_right = x + w - 210;
    let bar_max_width = 180;
    let bar_height = 16;

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

        let bar_fill_width =
            ((entry.total_minutes * bar_max_width as i64) / max_visible_minutes).max(2) as i32;
        let bar_top = row_y + ROW_TEXT_Y_OFFSET - (bar_height / 2);
        area.draw(&Rectangle::new(
            [
                (bar_right - bar_max_width, bar_top),
                (bar_right, bar_top + bar_height),
            ],
            ShapeStyle::from(&PANEL_STROKE.mix(0.9)).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw row bar bg: {:?}", e))?;
        area.draw(&Rectangle::new(
            [
                (bar_right - bar_max_width, bar_top),
                (bar_right - bar_max_width + bar_fill_width, bar_top + bar_height),
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
