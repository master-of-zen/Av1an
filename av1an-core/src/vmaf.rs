use crate::{read_vmaf_file, read_weighted_vmaf};
use plotters::prelude::*;
use std::{path::PathBuf, u32};

pub fn plot_vmaf_score_file(
  scores_file: PathBuf,
  plot_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
  let scores = read_vmaf_file(scores_file.clone()).unwrap();

  let plot_width = 2560 + ((scores.len() as f64).log10() as u32 * 200);
  let plot_heigth = 1440;

  let length = scores.len() as u32;
  let perc_1 = read_weighted_vmaf(scores_file.clone(), 0.01).unwrap();
  let perc_25 = read_weighted_vmaf(scores_file.clone(), 0.25).unwrap();
  let perc_75 = read_weighted_vmaf(scores_file.clone(), 0.75).unwrap();
  let perc_mean = read_weighted_vmaf(scores_file.clone(), 0.50).unwrap();

  let root =
    BitMapBackend::new(plot_path.as_os_str(), (plot_width, plot_heigth)).into_drawing_area();

  root.fill(&WHITE)?;

  let mut chart = ChartBuilder::on(&root)
    .set_label_area_size(LabelAreaPosition::Bottom, (8).percent())
    .set_label_area_size(LabelAreaPosition::Left, (5).percent())
    .set_label_area_size(LabelAreaPosition::Left, (5).percent())
    .set_label_area_size(LabelAreaPosition::Top, (5).percent())
    .margin((1).percent())
    .build_cartesian_2d(0u32..length, perc_1.floor()..100.0)?;

  chart.configure_mesh().draw()?;

  // 1%
  chart
    .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_1)), &RED))?
    .label(format!("1%: {}", perc_1))
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));

  // 25%
  chart
    .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_25)), &YELLOW))?
    .label(format!("25%: {}", perc_25))
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &YELLOW));

  // 75%
  chart
    .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_75)), &GREEN))?
    .label(format!("75%: {}", perc_75))
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &GREEN));

  // Mean
  chart
    .draw_series(LineSeries::new(
      (0..=length).map(|x| (x, perc_mean)),
      &BLACK,
    ))?
    .label(format!("Mean: {}", perc_mean))
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &BLACK));

  // Data
  chart.draw_series(LineSeries::new(
    (0..).zip(scores.iter()).map(|(x, y)| (x, *y)),
    &BLUE,
  ))?;

  chart
    .configure_series_labels()
    .background_style(&WHITE.mix(0.8))
    .border_style(&BLACK)
    .draw()?;

  root.present().expect("Unable to write result plot to file");

  Ok(())
}
