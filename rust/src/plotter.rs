use std::fs;

use crate::{commands::run, config::get_stats_dir};
use chrono::DateTime;
use plotters::prelude::*;
use plotters::style::{Palette, Palette99, ShapeStyle};
use polars::{datatypes::StaticArray, prelude::*};

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
    // let last_date = df.column("commit_time").unwrap().tail(Some(1));
    let commit_hashes = df.drop_in_place("commit_hash").unwrap();
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

    println!("{min_value}-{max_value}");
    let root = BitMapBackend::new(&image_path, (1500, 1200)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "{repo_name}:{stat_name} by repotracer at {}",
                run_at.to_rfc3339()
            ),
            ("sans-serif", 20).into_font(),
        )
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(30)
        .build_cartesian_2d(start_time..end_time, min_value..max_value)?;

    println!("ABOUT TO CHART");
    chart.configure_mesh().draw()?;

    let x_data = time_series.into_no_null_iter().collect::<Vec<_>>();
    let palette_size = Palette99::COLORS.len(); // Get the size of the palette
    let mut color_index = 0; // Initialize a color index

    for series in df.get_columns() {
        let name = series.name();
        println!("CHARTING {name}, {}", series.dtype());
        if !series.dtype().is_numeric() {
            println!(
                "Skipping plotting series {name} as its type {} is not numeric.",
                series.dtype()
            );
        }
        if name == "commit" || name == "commit_time" {
            continue;
        }
        let values: Vec<f64> = series
            .cast(&DataType::Float64)
            .unwrap()
            .f64()
            .unwrap()
            .into_no_null_iter()
            .collect();

        let data = x_data.iter().zip(values.iter()).map(|(&x, &y)| (x, y));

        println!("generating lineseries");

        let color = Palette99::pick(color_index % palette_size);
        color_index += 1; // Move to the next color for the next series

        let style = ShapeStyle::from(color).filled();
        chart
            .draw_series(LineSeries::new(data, style))?
            .label(format!("{}", name))
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], style.clone()));

        println!("DRAWING");
        chart
            .configure_series_labels()
            .background_style(&WHITE.mix(0.8))
            .border_style(&BLACK)
            .draw()?;
    }
    root.present()?;
    Ok(())
}
