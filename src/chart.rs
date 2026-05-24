use crate::db::{ChartData, LeaderboardEntry};
use plotters::prelude::*;

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

// Deep dark theme
const BG: RGBColor = RGBColor(0x0a, 0x0b, 0x0f);
const FG: RGBColor = RGBColor(0xe2, 0xe4, 0xe9);
const MUTED: RGBColor = RGBColor(0x44, 0x4a, 0x5c);

// Neon accent palette
const PALETTE: [RGBColor; 5] = [
    RGBColor(0x00, 0xd4, 0xff), // cyan
    RGBColor(0xb3, 0x4d, 0xff), // violet
    RGBColor(0xff, 0x2d, 0x6f), // pink
    RGBColor(0x00, 0xff, 0x87), // mint
    RGBColor(0xff, 0xa5, 0x00), // amber
];

// Leaderboard card colours
const LB_GOLD: RGBColor = RGBColor(0xff, 0xa5, 0x00);
const LB_SILVER: RGBColor = RGBColor(0xb0, 0xb8, 0xc8);
const LB_BRONZE: RGBColor = RGBColor(0xb8, 0x73, 0x33);
const LB_BAR_BG: RGBColor = RGBColor(0x1e, 0x21, 0x2e);
const LB_BAR_HI: RGBColor = RGBColor(0x00, 0xd4, 0xff); // bright cyan – top 3
const LB_BAR_LO: RGBColor = RGBColor(0x00, 0x72, 0x8a); // dimmer teal – rest

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
        ChartMode::Both => (1200, 960),
        _ => (1200, 520),
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
                draw_panel(&sub[0], data, false, "Weekly Hours — Top 5")?;
                draw_panel(&sub[1], data, true, "Cumulative Hours — Top 5")?;
            }
            ChartMode::Cumulative => {
                draw_panel(&root, data, true, "Cumulative Hours — Top 5")?;
            }
            ChartMode::Totals => {
                draw_panel(&root, data, false, "Weekly Hours — Top 5")?;
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
        .caption(title, ("sans-serif", 20).into_font().color(&FG))
        .margin(25)
        .x_label_area_size(42)
        .y_label_area_size(52)
        .build_cartesian_2d(0i32..x_max, 0.0f64..y_max)
        .map_err(|e| anyhow::anyhow!("build chart: {:?}", e))?;

    let week_labels = &data.week_labels;
    chart
        .configure_mesh()
        .disable_x_mesh()
        .disable_y_mesh()
        .axis_style(MUTED.mix(0.0)) // fully transparent — no visible axis line
        .x_labels(label_count)
        .y_labels(4)
        .x_label_style(("sans-serif", 11).into_font().color(&MUTED))
        .x_label_formatter(&|x| {
            week_labels
                .get(*x as usize)
                .map(|s| short_week_label(s))
                .unwrap_or_default()
        })
        .y_label_style(("sans-serif", 11).into_font().color(&MUTED))
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

        // Glow line — wide outer halo (18px, 6% opacity)
        chart
            .draw_series(LineSeries::new(
                points.clone(),
                color.mix(0.06).stroke_width(18),
            ))
            .map_err(|e| anyhow::anyhow!("draw glow1: {:?}", e))?;

        // Glow line — medium halo (8px, 18% opacity)
        chart
            .draw_series(LineSeries::new(
                points.clone(),
                color.mix(0.18).stroke_width(8),
            ))
            .map_err(|e| anyhow::anyhow!("draw glow2: {:?}", e))?;

        // Main line (2px, 100% opacity) — carries label and legend
        let username = user.username.clone();
        chart
            .draw_series(LineSeries::new(points.clone(), color.stroke_width(2)))
            .map_err(|e| anyhow::anyhow!("draw line: {:?}", e))?
            .label(username)
            .legend(move |(lx, ly)| {
                PathElement::new(vec![(lx, ly), (lx + 20, ly)], color.stroke_width(2))
            });

        // Glow endpoint dot at the last data point.
        if let Some(&last_pt) = points.last() {
            chart
                .draw_series(std::iter::once(Circle::new(
                    last_pt,
                    9,
                    color.mix(0.10).filled(),
                )))
                .map_err(|e| anyhow::anyhow!("draw dot1: {:?}", e))?;
            chart
                .draw_series(std::iter::once(Circle::new(
                    last_pt,
                    5,
                    color.mix(0.30).filled(),
                )))
                .map_err(|e| anyhow::anyhow!("draw dot2: {:?}", e))?;
            chart
                .draw_series(std::iter::once(Circle::new(last_pt, 2, color.filled())))
                .map_err(|e| anyhow::anyhow!("draw dot3: {:?}", e))?;
        }
    }

    chart
        .configure_series_labels()
        .background_style(RGBAColor(0, 0, 0, 0.0))
        .border_style(RGBAColor(0, 0, 0, 0.0))
        .label_font(("sans-serif", 12).into_font().color(&FG))
        .position(SeriesLabelPosition::LowerLeft)
        .draw()
        .map_err(|e| anyhow::anyhow!("draw legend: {:?}", e))?;

    Ok(())
}

/// Format minutes as "Xh Ym" (or "Ym" when hours == 0).
fn fmt_dur(minutes: i64) -> String {
    let h = minutes / 60;
    let m = minutes % 60;
    if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m", m)
    }
}

/// Render a dark premium leaderboard card for `/clock leaderboard`.
///
/// Produces a PNG image with two sections (weekly + all-time), each showing
/// rank, username, a horizontal progress bar, and the duration.
pub fn render_leaderboard_card(
    weekly: &[LeaderboardEntry],
    alltime: &[LeaderboardEntry],
    week_label: &str,
    timestamp: &str,
) -> anyhow::Result<Vec<u8>> {
    // ── Layout constants (all in pixels) ────────────────────────────────────
    const CARD_W: u32 = 860;
    const MX: i32 = 44; // left / right margin

    // Column x positions
    const COL_RANK: i32 = MX;           // "#1"  starts here
    const COL_NAME: i32 = MX + 52;      // username starts here
    const COL_BAR: i32 = MX + 52 + 168; // bar starts here
    const BAR_W: i32 = 420;             // bar width
    const BAR_H: i32 = 8;              // bar height
    const COL_DUR: i32 = COL_BAR + BAR_W + 14; // duration starts here

    // Vertical block heights
    const TITLE_BLOCK: i32 = 72;
    const SEC_BLOCK: i32 = 44;
    const ROW_H: i32 = 34;
    const TOTAL_BLOCK: i32 = 36;
    const GAP_BLOCK: i32 = 28;
    const FOOTER_BLOCK: i32 = 48;

    // Rows displayed per section (≤10 each)
    let n1 = weekly.len().min(10);
    let n2 = alltime.len().min(10);
    // Display at least 1 row per section (for the "no data" placeholder)
    let n1_h = n1.max(1) as i32;
    let n2_h = n2.max(1) as i32;

    let height: u32 = (TITLE_BLOCK
        + SEC_BLOCK + n1_h * ROW_H + TOTAL_BLOCK
        + GAP_BLOCK
        + SEC_BLOCK + n2_h * ROW_H + TOTAL_BLOCK
        + FOOTER_BLOCK) as u32;

    // ── Render ───────────────────────────────────────────────────────────────
    let mut pixel_buf = vec![0u8; (CARD_W * height * 3) as usize];
    {
        let root =
            BitMapBackend::with_buffer(&mut pixel_buf, (CARD_W, height)).into_drawing_area();
        root.fill(&BG)
            .map_err(|e| anyhow::anyhow!("fill: {:?}", e))?;

        // Title
        root.draw_text(
            "Leaderboard",
            &("sans-serif", 24).into_font().color(&FG),
            (MX, 20),
        )
        .map_err(|e| anyhow::anyhow!("title: {:?}", e))?;

        // Divider below title
        root.draw(&Rectangle::new(
            [(MX, TITLE_BLOCK - 4), (CARD_W as i32 - MX, TITLE_BLOCK - 3)],
            MUTED.mix(0.3).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("div title: {:?}", e))?;

        // ── Section 1 – This week ────────────────────────────────────────────
        let weekly_slice = &weekly[..n1];
        let max1 = weekly_slice.iter().map(|e| e.total_minutes).max().unwrap_or(1);
        let weekly_total: i64 = weekly_slice.iter().map(|e| e.total_minutes).sum();

        let sec1_y = TITLE_BLOCK;
        root.draw_text(
            &format!("This week  ·  {}", week_label),
            &("sans-serif", 14).into_font().color(&LB_GOLD),
            (MX, sec1_y + 10),
        )
        .map_err(|e| anyhow::anyhow!("sec1 label: {:?}", e))?;

        let rows1_y = sec1_y + SEC_BLOCK;
        draw_lb_rows(
            &root,
            weekly_slice,
            max1,
            rows1_y,
            ROW_H,
            BAR_H,
            COL_RANK,
            COL_NAME,
            COL_BAR,
            BAR_W,
            COL_DUR,
        )?;

        let total1_y = rows1_y + n1_h * ROW_H;
        root.draw_text(
            &format!("Total  {}", fmt_dur(weekly_total)),
            &("sans-serif", 13).into_font().color(&MUTED),
            (COL_NAME, total1_y + 8),
        )
        .map_err(|e| anyhow::anyhow!("total1: {:?}", e))?;

        // ── Gap + divider ────────────────────────────────────────────────────
        let gap_y = total1_y + TOTAL_BLOCK;
        root.draw(&Rectangle::new(
            [
                (MX, gap_y + GAP_BLOCK / 2 - 1),
                (CARD_W as i32 - MX, gap_y + GAP_BLOCK / 2),
            ],
            MUTED.mix(0.2).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("div gap: {:?}", e))?;

        // ── Section 2 – All time ─────────────────────────────────────────────
        let alltime_slice = &alltime[..n2];
        let max2 = alltime_slice.iter().map(|e| e.total_minutes).max().unwrap_or(1);
        let alltime_total: i64 = alltime_slice.iter().map(|e| e.total_minutes).sum();

        let sec2_y = gap_y + GAP_BLOCK;
        root.draw_text(
            "All time",
            &("sans-serif", 14).into_font().color(&LB_GOLD),
            (MX, sec2_y + 10),
        )
        .map_err(|e| anyhow::anyhow!("sec2 label: {:?}", e))?;

        let rows2_y = sec2_y + SEC_BLOCK;
        draw_lb_rows(
            &root,
            alltime_slice,
            max2,
            rows2_y,
            ROW_H,
            BAR_H,
            COL_RANK,
            COL_NAME,
            COL_BAR,
            BAR_W,
            COL_DUR,
        )?;

        let total2_y = rows2_y + n2_h * ROW_H;
        root.draw_text(
            &format!("Total  {}", fmt_dur(alltime_total)),
            &("sans-serif", 13).into_font().color(&MUTED),
            (COL_NAME, total2_y + 8),
        )
        .map_err(|e| anyhow::anyhow!("total2: {:?}", e))?;

        // ── Footer ───────────────────────────────────────────────────────────
        let footer_y = total2_y + TOTAL_BLOCK;
        root.draw(&Rectangle::new(
            [(MX, footer_y + 8), (CARD_W as i32 - MX, footer_y + 9)],
            MUTED.mix(0.2).filled(),
        ))
        .map_err(|e| anyhow::anyhow!("div footer: {:?}", e))?;

        root.draw_text(
            &format!("{}  ·  Resets every Monday 00:00", timestamp),
            &("sans-serif", 11).into_font().color(&MUTED),
            (MX, footer_y + 16),
        )
        .map_err(|e| anyhow::anyhow!("footer text: {:?}", e))?;

        root.present()
            .map_err(|e| anyhow::anyhow!("present: {:?}", e))?;
    }

    // Encode pixel buffer to PNG
    let img = image::RgbImage::from_raw(CARD_W, height, pixel_buf)
        .ok_or_else(|| anyhow::anyhow!("failed to create RGB image from pixel buffer"))?;
    let mut png_bytes: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| anyhow::anyhow!("PNG encode: {}", e))?;
    Ok(png_bytes)
}

/// Draw leaderboard rows into `area` starting at the given `start_y` pixel.
fn draw_lb_rows<DB>(
    area: &DrawingArea<DB, plotters::coord::Shift>,
    entries: &[LeaderboardEntry],
    max_minutes: i64,
    start_y: i32,
    row_h: i32,
    bar_h: i32,
    col_rank: i32,
    col_name: i32,
    col_bar: i32,
    bar_w: i32,
    col_dur: i32,
) -> anyhow::Result<()>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    if entries.is_empty() {
        area.draw_text(
            "no data yet",
            &("sans-serif", 13).into_font().color(&MUTED),
            (col_name, start_y + (row_h - 13) / 2),
        )
        .map_err(|e| anyhow::anyhow!("no data text: {:?}", e))?;
        return Ok(());
    }

    for (i, entry) in entries.iter().enumerate() {
        let row_top = start_y + i as i32 * row_h;
        // Vertically centre 14 px text within the row
        let text_y = row_top + (row_h - 14) / 2;

        // Rank colour: gold / silver / bronze for top 3, muted for the rest
        let rank_color = match i {
            0 => LB_GOLD,
            1 => LB_SILVER,
            2 => LB_BRONZE,
            _ => MUTED,
        };
        area.draw_text(
            &format!("#{}", i + 1),
            &("sans-serif", 14).into_font().color(&rank_color),
            (col_rank, text_y),
        )
        .map_err(|e| anyhow::anyhow!("rank text: {:?}", e))?;

        // Username – truncate to 16 chars with ellipsis
        const MAX_NAME: usize = 16;
        let name: String = if entry.username.chars().count() > MAX_NAME {
            format!(
                "{}…",
                entry.username.chars().take(MAX_NAME - 1).collect::<String>()
            )
        } else {
            entry.username.clone()
        };
        area.draw_text(
            &name,
            &("sans-serif", 14).into_font().color(&FG),
            (col_name, text_y),
        )
        .map_err(|e| anyhow::anyhow!("name text: {:?}", e))?;

        // Bar background
        let bar_top = row_top + (row_h - bar_h) / 2;
        area.draw(&Rectangle::new(
            [(col_bar, bar_top), (col_bar + bar_w, bar_top + bar_h)],
            LB_BAR_BG.filled(),
        ))
        .map_err(|e| anyhow::anyhow!("bar bg: {:?}", e))?;

        // Bar fill – top 3 get the bright cyan; the rest get a dimmer teal
        let ratio = if max_minutes > 0 {
            (entry.total_minutes as f64 / max_minutes as f64).min(1.0)
        } else {
            0.0
        };
        let fill_px = ((ratio * bar_w as f64).round() as i32).max(0);
        if fill_px > 0 {
            let bar_color = if i < 3 { LB_BAR_HI } else { LB_BAR_LO };
            area.draw(&Rectangle::new(
                [(col_bar, bar_top), (col_bar + fill_px, bar_top + bar_h)],
                bar_color.filled(),
            ))
            .map_err(|e| anyhow::anyhow!("bar fill: {:?}", e))?;
        }

        // Duration
        area.draw_text(
            &fmt_dur(entry.total_minutes),
            &("sans-serif", 13).into_font().color(&FG),
            (col_dur, text_y),
        )
        .map_err(|e| anyhow::anyhow!("dur text: {:?}", e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ChartData, UserWeeklyData};

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
        use crate::db::LeaderboardEntry;
        let weekly = vec![
            LeaderboardEntry { username: "Alice".to_string(), total_minutes: 3980 },
            LeaderboardEntry { username: "Bob".to_string(), total_minutes: 517 },
        ];
        let alltime = vec![
            LeaderboardEntry { username: "Alice".to_string(), total_minutes: 9820 },
            LeaderboardEntry { username: "Bob".to_string(), total_minutes: 4200 },
            LeaderboardEntry { username: "Charlie".to_string(), total_minutes: 1800 },
        ];
        let bytes = render_leaderboard_card(&weekly, &alltime, "KW21/2026", "24.05.2026 10:00")
            .expect("render_leaderboard_card failed");
        assert!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "output is not a PNG"
        );
        assert!(bytes.len() > 1000, "PNG seems too small");
    }

    #[test]
    fn test_render_leaderboard_card_empty_data() {
        register_test_font();
        // Should not panic / error when both lists are empty
        let bytes =
            render_leaderboard_card(&[], &[], "KW21/2026", "24.05.2026 10:00")
                .expect("render_leaderboard_card with empty data failed");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
