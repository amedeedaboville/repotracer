use std::fs;

use crate::collectors::cached_walker::CommitData;
use crate::config::get_stats_dir;
use ahash::HashMap;
use chrono::{TimeZone, Utc};

use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};
use plotters::style::{Color, Palette};

struct SeabornDeepPalette;
impl Palette for SeabornDeepPalette {
    const COLORS: &'static [(u8, u8, u8)] = &[
        (76, 114, 176),  // Blue
        (85, 168, 104),  // Green
        (196, 78, 82),   // Red
        (129, 114, 178), // Purple
        (204, 185, 116), // Yellow
        (100, 181, 205), // Cyan
        //The rest are palette99
        (230, 25, 75),
        (60, 180, 75),
        (255, 225, 25),
        (0, 130, 200),
        (245, 130, 48),
        (145, 30, 180),
        (70, 240, 240),
        (240, 50, 230),
        (210, 245, 60),
        (250, 190, 190),
        (0, 128, 128),
        (230, 190, 255),
        (170, 110, 40),
        (255, 250, 200),
        (128, 0, 0),
        (170, 255, 195),
        (128, 128, 0),
        (255, 215, 180),
        (0, 0, 128),
        (128, 128, 128),
        (0, 0, 0),
    ];

    fn pick(idx: usize) -> PaletteColor<Self>
    where
        Self: Sized,
    {
        PaletteColor::<Self>::pick(idx)
    }
}

const IMAGE_WIDTH: u32 = 1800;
const IMAGE_HEIGHT: u32 = 1000;
pub fn plot(
    repo_name: &str,
    stat_name: &str,
    df: &Vec<CommitData>,
    stat_description: &str,
    run_at: &chrono::DateTime<chrono::Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo_stats_dir = get_stats_dir().join(repo_name);
    if !repo_stats_dir.exists() {
        fs::create_dir_all(&repo_stats_dir)?;
    }
    let image_path = repo_stats_dir.join(format!("{stat_name}.svg"));
    let commit_times: Vec<_> = df.iter().map(|c| c.date).collect();
    let start_time = commit_times.iter().min().unwrap().to_owned();
    let end_time = commit_times.iter().max().unwrap().to_owned();

    // Convert String values to f64 and find min/max
    let parsed_data: Vec<HashMap<String, f64>> = df.iter().map(|c| parse_stats(&c.data)).collect();

    let min_value: f64 = parsed_data
        .iter()
        .flat_map(|map| map.values())
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap()
        .to_owned();
    let max_value: f64 = parsed_data
        .iter()
        .flat_map(|map| map.values())
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap()
        .to_owned();

    let title = stat_description;
    let subtitle = format!(
        "{repo_name}:{stat_name} by repotracer at {}",
        run_at.format("%Y-%m-%d %H:%M:%S")
    );
    let text_gray = BLACK.mix(0.4);
    let root = SVGBackend::new(&image_path, (IMAGE_WIDTH, IMAGE_HEIGHT)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
        .caption(title, ("sans-serif", (4).percent_height()))
        .margin(5.percent())
        .margin_bottom(3.percent())
        .set_label_area_size(LabelAreaPosition::Left, (5).percent())
        .set_label_area_size(LabelAreaPosition::Bottom, (5).percent())
        .build_cartesian_2d(start_time..end_time, min_value..max_value)?;

    chart
        .configure_mesh()
        .max_light_lines(0)
        .label_style(("sans-serif", 3.percent_height()))
        .x_desc("Date")
        .axis_desc_style(TextStyle::from(("sans-serif", 25).into_font()).color(&text_gray))
        .y_label_formatter(&|y| format!("{:.0}", y))
        .x_label_formatter(&|x| x.format("%Y-%m-%d").to_string())
        // .x_label_formatter(&|x| {
        //     let seconds = *x / 1000;
        //     let naive = chrono::NaiveDateTime::from_timestamp_opt(seconds, 0).unwrap();
        //     let datetime: chrono::DateTime<chrono::Utc> =
        //         chrono::DateTime::from_naive_utc_and_offset(naive, chrono::Utc);
        //     format!("{}", datetime.format("%Y-%m-%d"))
        // })
        .draw()?;

    let description_style = TextStyle::from(("sans-serif", 25).into_font())
        .color(&text_gray)
        .pos(Pos::new(HPos::Right, VPos::Top));
    root.draw_text(
        &subtitle,
        &description_style,
        ((IMAGE_WIDTH - 100) as i32, 50),
    )?;
    let palette_size = SeabornDeepPalette::COLORS.len(); // Get the size of the palette

    for (idx, stat_key) in df[0].data.keys().enumerate() {
        let values: Vec<f64> = parsed_data
            .iter()
            .map(|map| *map.get(stat_key).unwrap_or(&0.0))
            .collect();

        let mut data: Vec<_> =
            commit_times
                .iter()
                .zip(values.iter())
                .map(|(&x, &y)| (x, y))
                .collect();
        data.dedup_by(|a, b| a == b);

        let color = SeabornDeepPalette::pick(idx % palette_size).mix(0.9);

        chart
            .draw_series(LineSeries::new(data.into_iter(), color.stroke_width(4)))?
            .label(stat_key)
            .legend(move |(x, y)| Rectangle::new([(x, y - 5), (x + 10, y + 5)], color.filled()));
    }

    chart
        .configure_series_labels()
        .label_font(("sans-serif", 20)) // Adjust the font size here
        .background_style(WHITE)
        .border_style(BLACK)
        .draw()?;
    root.present()?;
    Ok(())
}

// Helper function to parse String values to f64
fn parse_stats(data: &HashMap<String, String>) -> HashMap<String, f64> {
    data.iter()
        .filter_map(|(key, value)| {
            value
                .parse::<f64>()
                .ok()
                .map(|parsed_value| (key.clone(), parsed_value))
        })
        .collect()
}
