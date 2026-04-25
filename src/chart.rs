use crate::db::ChartData;
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

// Discord-dark friendly theme
const BG: RGBColor = RGBColor(0x1e, 0x1f, 0x22); // panel background
const FG: RGBColor = RGBColor(0xea, 0xec, 0xee); // primary text
const MUTED: RGBColor = RGBColor(0x9a, 0xa0, 0xa6); // axis text / subtitle
const GRID: RGBColor = RGBColor(0x2f, 0x33, 0x39); // gridlines / axis line

// Tableau-10-ish, high contrast on dark, color-blind friendlier than the old palette
const PALETTE: [RGBColor; 5] = [
    RGBColor(0x4C, 0x9A, 0xFF), // blue
    RGBColor(0xF5, 0x85, 0x18), // orange
    RGBColor(0x54, 0xA2, 0x4B), // green
    RGBColor(0xE4, 0x57, 0x56), // red
    RGBColor(0x72, 0xB7, 0xB2), // teal
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
        ChartMode::Both => (1200, 900),
        _ => (1200, 600),
    };

    // Render to a raw RGB pixel buffer (3 bytes per pixel).
    let mut pixel_buf = vec![0u8; (width * height * 3) as usize];
    {
        let root =
            BitMapBackend::with_buffer(&mut pixel_buf, (width, height)).into_drawing_area();
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
            .map(|u| u.minutes_per_week.iter().map(|&m| m as f64 / 60.0).sum::<f64>())
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
        .margin(25)
        .x_label_area_size(42)
        .y_label_area_size(52)
        .build_cartesian_2d(0i32..x_max, 0.0f64..y_max)
        .map_err(|e| anyhow::anyhow!("build chart: {:?}", e))?;

    let week_labels = &data.week_labels;
    chart
        .configure_mesh()
        .disable_x_mesh()
        .light_line_style(GRID)
        .bold_line_style(GRID)
        .axis_style(GRID)
        .x_labels(label_count)
        .x_label_style(("sans-serif", 11).into_font().color(&MUTED))
        .x_label_formatter(&|x| {
            week_labels
                .get(*x as usize)
                .map(|s| short_week_label(s))
                .unwrap_or_default()
        })
        .y_desc("Hours")
        .axis_desc_style(("sans-serif", 12).into_font().color(&MUTED))
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

        // For cumulative mode, draw a translucent area fill under the line.
        if cumulative {
            let mut area_pts: Vec<(i32, f64)> = points.clone();
            // Close the polygon along the baseline.
            if let (Some(&(last_x, _)), Some(&(first_x, _))) = (points.last(), points.first()) {
                area_pts.push((last_x, 0.0));
                area_pts.push((first_x, 0.0));
            }
            chart
                .draw_series(std::iter::once(Polygon::new(area_pts, color.mix(0.10).filled())))
                .map_err(|e| anyhow::anyhow!("draw area: {:?}", e))?;
        }

        let username = user.username.clone();
        chart
            .draw_series(LineSeries::new(points.clone(), color.stroke_width(4)))
            .map_err(|e| anyhow::anyhow!("draw line: {:?}", e))?
            .label(username)
            .legend(move |(lx, ly)| {
                PathElement::new(vec![(lx, ly), (lx + 20, ly)], color.stroke_width(4))
            });

        // Draw a single endpoint dot at the last data point.
        if let Some(&last_pt) = points.last() {
            chart
                .draw_series(std::iter::once(Circle::new(last_pt, 5, color.filled())))
                .map_err(|e| anyhow::anyhow!("draw endpoint: {:?}", e))?;
        }
    }

    chart
        .configure_series_labels()
        .background_style(BG.mix(0.85))
        .border_style(GRID)
        .label_font(("sans-serif", 12).into_font().color(&FG))
        .position(SeriesLabelPosition::UpperRight)
        .draw()
        .map_err(|e| anyhow::anyhow!("draw legend: {:?}", e))?;

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
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "output is not a PNG");
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
}
