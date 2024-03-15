use std::fs;

use crate::config::get_stats_dir;
use plotters::prelude::*;
use plotters::style::{Color, Palette};
use polars::prelude::*;

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

pub fn plot(
    repo_name: &str,
    stat_name: &str,
    df: &mut DataFrame,
    stat_description: &str,
    run_at: &chrono::DateTime<chrono::Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo_stats_dir = get_stats_dir().join(repo_name);
    if !repo_stats_dir.exists() {
        fs::create_dir_all(&repo_stats_dir)?;
    }
    let image_path = repo_stats_dir.join(format!("{stat_name}.png"));
    let _commit_hashes = df.drop_in_place("commit_hash").unwrap();
    let commit_times = df.drop_in_place("commit_time").unwrap();
    let time_series = commit_times.datetime().unwrap();
    let start_time = time_series.min().unwrap();
    let end_time = time_series.max().unwrap();

    let min_value: f64 = df
        .clone()
        .lazy()
        .min()?
        .collect()?
        .min_horizontal()?
        .unwrap()
        .get(0)?
        .try_extract()?;
    let max_value: f64 = df
        .clone()
        .lazy()
        .max()?
        .collect()?
        .max_horizontal()?
        .unwrap()
        .get(0)?
        .try_extract()?;

    let title = stat_description;
    let subtitle = format!(
        "{repo_name}:{stat_name} by repotracer at {}",
        run_at.format("%Y-%m-%d %H:%M:%S")
    );
    let text_gray = BLACK.mix(0.4);
    let root = SVGBackend::new(&image_path, (1200, 1000)).into_drawing_area();
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
        .x_label_formatter(&|x| {
            let seconds = *x / 1000;
            let naive = chrono::NaiveDateTime::from_timestamp(seconds, 0);
            let datetime: chrono::DateTime<chrono::Utc> =
                chrono::DateTime::from_utc(naive, chrono::Utc);
            format!("{}", datetime.format("%Y-%m-%d"))
        })
        .draw()?;

    let description_style = TextStyle::from(("sans-serif", 25).into_font()).color(&text_gray);
    root.draw_text(&subtitle, &description_style, (50, 50))?;
    let x_data = time_series.into_no_null_iter().collect::<Vec<_>>();
    let palette_size = SeabornDeepPalette::COLORS.len(); // Get the size of the palette

    for (idx, series) in df.get_columns().iter().enumerate() {
        if !series.dtype().is_numeric() {
            println!(
                "Skipping plotting series {} as its type {} is not numeric.",
                series.name(),
                series.dtype()
            );
        }
        let values: Vec<f64> = series
            .cast(&DataType::Float64)
            .unwrap()
            .f64()
            .unwrap()
            .into_no_null_iter()
            .collect();

        let data = x_data.iter().zip(values.iter()).map(|(&x, &y)| (x, y));
        let color = SeabornDeepPalette::pick(idx % palette_size).mix(0.9);

        chart
            .draw_series(LineSeries::new(data, color.stroke_width(4)))?
            .label(series.name())
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
