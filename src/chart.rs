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

/// Five distinct colors for the up-to-five user lines.
const LINE_COLORS: [RGBColor; 5] = [
    RGBColor(0x58, 0x65, 0xF2), // Discord blurple
    RGBColor(0x2E, 0xCC, 0x71), // Emerald green
    RGBColor(0xE7, 0x4C, 0x3C), // Alizarin red
    RGBColor(0xF1, 0xC4, 0x0F), // Sunflower yellow
    RGBColor(0x9B, 0x59, 0xB6), // Amethyst purple
];

/// Render a line chart for `data` according to `mode`.
/// Returns raw PNG bytes.
pub fn render_chart(data: &ChartData, mode: ChartMode) -> anyhow::Result<Vec<u8>> {
    let n_weeks = data.week_labels.len();
    if n_weeks < 2 || data.users.is_empty() {
        anyhow::bail!("not enough data to render chart");
    }

    // Write to a temp file then read back as bytes.
    let tmp_path = std::env::temp_dir().join(format!(
        "clock_chart_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let tmp_str = tmp_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("temp path is not valid UTF-8"))?
        .to_owned();

    let (width, height): (u32, u32) = match mode {
        ChartMode::Both => (1200, 900),
        _ => (1200, 600),
    };

    {
        let root = BitMapBackend::new(&tmp_str, (width, height)).into_drawing_area();
        root.fill(&WHITE)
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

    let bytes = std::fs::read(&tmp_path)?;
    let _ = std::fs::remove_file(&tmp_path);
    Ok(bytes)
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
    let y_max: f64 = if cumulative {
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
    let y_max = (y_max * 1.15).max(1.0).ceil();

    let x_max = (n_weeks - 1) as i32;

    // Decide how many X tick labels to show (avoid crowding).
    let label_count = if n_weeks <= 12 { n_weeks } else { 6 };

    let mut chart = ChartBuilder::on(area)
        .caption(title, ("sans-serif", 18).into_font())
        .margin(15)
        .x_label_area_size(50)
        .y_label_area_size(55)
        .build_cartesian_2d(0i32..x_max, 0.0f64..y_max)
        .map_err(|e| anyhow::anyhow!("build chart: {:?}", e))?;

    let week_labels = &data.week_labels;
    chart
        .configure_mesh()
        .x_labels(label_count)
        .x_label_formatter(&|x| {
            week_labels
                .get(*x as usize)
                .map(|s| s.as_str())
                .unwrap_or("")
                .to_owned()
        })
        .y_desc("Hours")
        .y_label_formatter(&|y| format!("{:.0}h", y))
        .draw()
        .map_err(|e| anyhow::anyhow!("configure mesh: {:?}", e))?;

    for (i, user) in data.users.iter().enumerate() {
        let color = LINE_COLORS[i % LINE_COLORS.len()];

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

        let username = user.username.clone();
        chart
            .draw_series(LineSeries::new(points.clone(), color.stroke_width(2)))
            .map_err(|e| anyhow::anyhow!("draw line: {:?}", e))?
            .label(username)
            .legend(move |(lx, ly)| {
                PathElement::new(vec![(lx, ly), (lx + 20, ly)], color.stroke_width(2))
            });

        // Draw dot markers at each data point.
        chart
            .draw_series(points.iter().map(|&(x, y)| Circle::new((x, y), 4, color.filled())))
            .map_err(|e| anyhow::anyhow!("draw circles: {:?}", e))?;
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.85))
        .border_style(BLACK)
        .position(SeriesLabelPosition::UpperLeft)
        .draw()
        .map_err(|e| anyhow::anyhow!("draw legend: {:?}", e))?;

    Ok(())
}
