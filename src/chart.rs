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
const BAR_BG: RGBColor = RGBColor(0x12, 0x14, 0x1b);
const WEEK_ACCENT: RGBColor = RGBColor(0xd8, 0xe2, 0xff);
const ALLTIME_ACCENT: RGBColor = RGBColor(0xa7, 0x8b, 0xfa);

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
        ChartMode::Both => (1400, 1080),
        _ => (1400, 620),
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
    let width: u32 = 1440;
    let height: u32 = 980;
    let mut pixel_buf = vec![0u8; (width * height * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut pixel_buf, (width, height)).into_drawing_area();
        root.fill(&BG)
            .map_err(|e| anyhow::anyhow!("fill error: {:?}", e))?;

        root.draw(&Rectangle::new(
            [(36, 32), (1404, 920)],
            ShapeStyle::from(&PANEL_BG).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw card bg: {:?}", e))?;
        root.draw(&Rectangle::new(
            [(36, 32), (1404, 920)],
            ShapeStyle::from(&PANEL_STROKE).stroke_width(1),
        ))
        .map_err(|e| anyhow::anyhow!("draw card border: {:?}", e))?;

        root.draw(&Text::new(
            "Leaderboard",
            (80, 92),
            ("sans-serif", 42).into_font().color(&FG),
        ))
        .map_err(|e| anyhow::anyhow!("draw title: {:?}", e))?;

        root.draw(&Text::new(
            "Weekly and all-time tracked hours",
            (80, 126),
            ("sans-serif", 19).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw subtitle: {:?}", e))?;

        root.draw(&PathElement::new(
            vec![(80, 156), (1360, 156)],
            PANEL_STROKE.mix(0.85).stroke_width(1),
        ))
        .map_err(|e| anyhow::anyhow!("draw header divider: {:?}", e))?;

        let section_w = 1280;
        let section_h = 330;
        let left = 80;
        let week_top = 186;
        let alltime_top = 534;

        draw_leaderboard_section(
            &root,
            left,
            week_top,
            section_w,
            section_h,
            &format!("This week · {}", week_label),
            weekly,
            WEEK_ACCENT,
        )?;
        draw_leaderboard_section(
            &root,
            left,
            alltime_top,
            section_w,
            section_h,
            "All time",
            alltime,
            ALLTIME_ACCENT,
        )?;

        root.draw(&Text::new(
            "Resets every Monday 00:00 · Europe/Zurich",
            (80, 954),
            ("sans-serif", 16).into_font().color(&MUTED),
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
    area.draw(&Text::new(
        title,
        (x, y + 34),
        ("sans-serif", 24).into_font().color(&FG),
    ))
    .map_err(|e| anyhow::anyhow!("draw section title: {:?}", e))?;

    let total_minutes: i64 = entries.iter().map(|e| e.total_minutes).sum();
    area.draw(&Text::new(
        format!("total · {}", format_duration(total_minutes)),
        (x + w, y + 34),
        ("sans-serif", 18)
            .into_font()
            .color(&MUTED)
            .pos(Pos::new(HPos::Right, VPos::Center)),
    ))
    .map_err(|e| anyhow::anyhow!("draw section total: {:?}", e))?;

    if entries.is_empty() {
        area.draw(&Text::new(
            "No data yet.",
            (x, y + 92),
            ("sans-serif", 18).into_font().color(&MUTED),
        ))
        .map_err(|e| anyhow::anyhow!("draw empty text: {:?}", e))?;
        return Ok(());
    }

    let rank_x = x;
    let name_x = x + 84;
    let bar_x = x + 376;
    let bar_w = w - 584;
    let dur_x = x + w;
    let row_start_y = y + 76;
    const HEADER_HEIGHT: i32 = 92;
    const ROW_HEIGHT: i32 = 42;
    const TOP_RANKS_COUNT: usize = 3;
    let max_entries = ((h - HEADER_HEIGHT) / ROW_HEIGHT).max(1) as usize;
    let visible = entries.iter().take(max_entries).collect::<Vec<_>>();
    let max_minutes = visible.iter().map(|e| e.total_minutes).max().unwrap_or(1);

    for (idx, entry) in visible.iter().enumerate() {
        let row_y = row_start_y + (idx as i32 * ROW_HEIGHT);
        let ratio = (entry.total_minutes as f64 / max_minutes.max(1) as f64).clamp(0.0, 1.0);
        let filled = ((bar_w as f64 * ratio).round() as i32).max(1);
        let rank_color = if idx < TOP_RANKS_COUNT { accent } else { MUTED };

        if idx > 0 {
            area.draw(&PathElement::new(
                vec![(x, row_y - 10), (x + w, row_y - 10)],
                PANEL_STROKE.mix(0.45).stroke_width(1),
            ))
            .map_err(|e| anyhow::anyhow!("draw row divider: {:?}", e))?;
        }

        area.draw(&Text::new(
            format!("#{}", idx + 1),
            (rank_x, row_y + 18),
            ("sans-serif", 18).into_font().color(&rank_color),
        ))
        .map_err(|e| anyhow::anyhow!("draw rank: {:?}", e))?;

        area.draw(&Text::new(
            truncate_name(&entry.username, 18),
            (name_x, row_y + 18),
            ("sans-serif", 18).into_font().color(&FG),
        ))
        .map_err(|e| anyhow::anyhow!("draw name: {:?}", e))?;

        area.draw(&Rectangle::new(
            [(bar_x, row_y + 7), (bar_x + bar_w, row_y + 17)],
            ShapeStyle::from(&BAR_BG.mix(0.9)).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw bar bg: {:?}", e))?;
        area.draw(&Rectangle::new(
            [(bar_x, row_y + 7), (bar_x + filled, row_y + 17)],
            ShapeStyle::from(&accent.mix(0.72)).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("draw bar fg: {:?}", e))?;

        area.draw(&Text::new(
            format_duration(entry.total_minutes),
            (dur_x, row_y + 18),
            ("sans-serif", 18)
                .into_font()
                .color(&FG)
                .pos(Pos::new(HPos::Right, VPos::Center)),
        ))
        .map_err(|e| anyhow::anyhow!("draw duration: {:?}", e))?;
    }

    Ok(())
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
    let card_left = 34;
    let card_top = 26;
    let card_right = area_w as i32 - 34;
    let card_bottom = area_h as i32 - 28;

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
    let label_count = n_weeks.min(8);

    let mut chart = ChartBuilder::on(area)
        .caption(title, ("sans-serif", 24).into_font().color(&FG))
        .margin(58)
        .x_label_area_size(46)
        .y_label_area_size(58)
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
        .y_labels(5)
        .x_label_style(("sans-serif", 13).into_font().color(&MUTED))
        .x_label_formatter(&|x| {
            week_labels
                .get(*x as usize)
                .map(|s| short_week_label(s))
                .unwrap_or_default()
        })
        .y_label_style(("sans-serif", 13).into_font().color(&MUTED))
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
        let username = user.username.clone();
        chart
            .draw_series(LineSeries::new(points.clone(), color.mix(0.82).stroke_width(2)))
            .map_err(|e| anyhow::anyhow!("draw line: {:?}", e))?
            .label(username)
            .legend(move |(lx, ly)| {
                PathElement::new(vec![(lx, ly), (lx + 22, ly)], color.mix(0.82).stroke_width(2))
            });

        if let Some(&last_pt) = points.last() {
            chart
                .draw_series(std::iter::once(Circle::new(
                    last_pt,
                    3,
                    color.mix(0.82).filled(),
                )))
                .map_err(|e| anyhow::anyhow!("draw endpoint: {:?}", e))?;
        }
    }

    chart
        .configure_series_labels()
        .background_style(PANEL_BG.mix(0.94).filled())
        .border_style(PANEL_STROKE.mix(0.6))
        .label_font(("sans-serif", 14).into_font().color(&FG))
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
        use plotters::style::{FontStyle, register_font};
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
        let bytes =
            render_leaderboard_card("KW21/2026", &weekly, &alltime).expect("render failed");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(bytes.len() > 10_000, "leaderboard card PNG seems too small");
    }
}
