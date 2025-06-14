use std::{
    cmp::Ordering,
    collections::HashMap,
    ffi::OsStr,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{anyhow, Context};
use plotters::prelude::*;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    broker::EncoderCrash,
    ffmpeg,
    ref_smallvec,
    util::printable_base10_digits,
    Input,
    ProbingStatistic,
    ProbingStatisticName,
};

#[derive(Deserialize, Serialize, Debug)]
struct VmafScore {
    vmaf: f64,
}

#[derive(Deserialize, Serialize, Debug)]
struct Metrics {
    metrics: VmafScore,
}

#[derive(Deserialize, Serialize, Debug)]
struct VmafResult {
    frames: Vec<Metrics>,
}

pub struct MetricStatistics {
    scores: Vec<f64>,
    cache:  HashMap<String, f64>,
}

impl MetricStatistics {
    pub fn new(scores: Vec<f64>) -> Self {
        MetricStatistics {
            scores,
            cache: HashMap::new(),
        }
    }

    fn get_or_compute(&mut self, key: &str, compute: impl FnOnce(&[f64]) -> f64) -> f64 {
        *self.cache.entry(key.to_string()).or_insert_with(|| compute(&self.scores))
    }

    pub fn mean(&mut self) -> f64 {
        self.get_or_compute("average", |scores| {
            scores.iter().sum::<f64>() / scores.len() as f64
        })
    }

    pub fn harmonic_mean(&mut self) -> f64 {
        self.get_or_compute("harmonic_mean", |scores| {
            let sum_reciprocals: f64 = scores.iter().map(|&x| 1.0 / x).sum();
            scores.len() as f64 / sum_reciprocals
        })
    }

    pub fn median(&mut self) -> f64 {
        let mut sorted_scores = self.scores.clone();
        sorted_scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));
        self.get_or_compute("median", |scores| {
            let mid = scores.len() / 2;
            if scores.len() % 2 == 0 {
                (sorted_scores[mid - 1] + sorted_scores[mid]) / 2.0
            } else {
                sorted_scores[mid]
            }
        })
    }

    pub fn mode(&mut self) -> f64 {
        let mut counts = HashMap::new();
        for score in &self.scores {
            // Round to nearest integer for fewer unique buckets
            let rounded_score = score.round() as i32;
            *counts.entry(rounded_score).or_insert(0) += 1;
        }
        let max_count = counts.values().copied().max().unwrap_or(0);
        self.get_or_compute("mode", |scores| {
            *scores
                .iter()
                .find(|score| counts[&(score.round() as i32)] == max_count)
                .unwrap_or(&0.0)
        })
    }

    pub fn minimum(&mut self) -> f64 {
        self.get_or_compute("minimum", |scores| {
            *scores.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0)
        })
    }

    pub fn maximum(&mut self) -> f64 {
        self.get_or_compute("maximum", |scores| {
            *scores.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0)
        })
    }

    pub fn variance(&mut self) -> f64 {
        let average = self.mean();
        self.get_or_compute("variance", |scores| {
            scores
                .iter()
                .map(|x| {
                    let diff = x - average;
                    diff * diff
                })
                .sum::<f64>()
                / scores.len() as f64
        })
    }

    pub fn standard_deviation(&mut self) -> f64 {
        let variance = self.variance();
        self.get_or_compute("standard_deviation", |_| variance.sqrt())
    }

    pub fn percentile(&mut self, index: usize) -> f64 {
        let mut sorted_scores = self.scores.clone();
        sorted_scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));
        self.get_or_compute(&format!("percentile_{index}"), |scores| {
            let index = (index as f64 / 100.0 * scores.len() as f64) as usize;
            *sorted_scores.get(index).unwrap_or(&sorted_scores[0])
        })
    }

    pub fn root_mean_square(&mut self) -> f64 {
        self.get_or_compute("root_mean_square", |scores| {
            let sum_of_squares: f64 = scores.iter().map(|&x| x * x).sum();
            (sum_of_squares / scores.len() as f64).sqrt()
        })
    }
}

pub fn plot_vmaf_score_file(scores_file: &Path, plot_path: &Path) -> anyhow::Result<()> {
    let scores = read_vmaf_file(scores_file).with_context(|| "Failed to parse VMAF file")?;

    let mut sorted_scores = scores.clone();
    sorted_scores.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));

    let plot_width = 1600 + (printable_base10_digits(scores.len()) * 200);
    let plot_heigth = 600;

    let length = scores.len() as u32;

    let root =
        SVGBackend::new(plot_path.as_os_str(), (plot_width, plot_heigth)).into_drawing_area();

    root.fill(&WHITE)?;

    let perc_1 = percentile_of_sorted(&sorted_scores, 0.01);
    let perc_25 = percentile_of_sorted(&sorted_scores, 0.25);
    let perc_50 = percentile_of_sorted(&sorted_scores, 0.50);
    let perc_75 = percentile_of_sorted(&sorted_scores, 0.75);

    let mut chart = ChartBuilder::on(&root)
        .set_label_area_size(LabelAreaPosition::Bottom, (5).percent())
        .set_label_area_size(LabelAreaPosition::Left, (5).percent())
        .set_label_area_size(LabelAreaPosition::Right, (7).percent())
        .set_label_area_size(LabelAreaPosition::Top, (5).percent())
        .margin((1).percent())
        .build_cartesian_2d(0_u32..length, perc_1.floor()..100.0)?;

    chart.configure_mesh().draw()?;

    // 1%
    chart
        .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_1)), RED))?
        .label(format!("1%: {perc_1}"))
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], RED));

    // 25%
    chart
        .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_25)), YELLOW))?
        .label(format!("25%: {perc_25}"))
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], YELLOW));

    // 50% (median, except not averaged in the case of an even number of elements)
    chart
        .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_50)), BLACK))?
        .label(format!("50%: {perc_50}"))
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], BLACK));

    // 75%
    chart
        .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_75)), GREEN))?
        .label(format!("75%: {perc_75}"))
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], GREEN));

    // Data
    chart.draw_series(LineSeries::new(
        (0..).zip(scores.iter()).map(|(x, y)| (x, *y)),
        BLUE,
    ))?;

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present().expect("Unable to write result plot to file");

    Ok(())
}

pub fn validate_libvmaf() -> anyhow::Result<()> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-h");

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let out = cmd.output()?;

    let stdr = String::from_utf8(out.stderr)?;
    if !stdr.contains("--enable-libvmaf") {
        return Err(anyhow!(
            "FFmpeg is not compiled with --enable-libvmaf, but target quality or VMAF plotting \
             was enabled"
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn plot(
    encoded: &Path,
    reference: &Input,
    model: Option<impl AsRef<Path>>,
    res: &str,
    scaler: &str,
    sample_rate: usize,
    filter: Option<&str>,
    threads: usize,
) -> Result<(), Box<EncoderCrash>> {
    let json_file = encoded.with_extension("json");
    let plot_file = encoded.with_extension("svg");
    let vspipe_args;

    println!(":: VMAF Run");

    let pipe_cmd: SmallVec<[&OsStr; 8]> = match reference {
        Input::Video {
            ref path,
        } => {
            vspipe_args = vec![];
            ref_smallvec!(OsStr, 8, [
                "ffmpeg",
                "-i",
                path,
                "-strict",
                "-1",
                "-f",
                "yuv4mpegpipe",
                "-"
            ])
        },
        Input::VapourSynth {
            ref path,
            vspipe_args: args,
        } => {
            vspipe_args = args.to_owned();
            ref_smallvec!(OsStr, 8, ["vspipe", "-c", "y4m", path, "-"])
        },
    };

    run_vmaf(
        encoded,
        &pipe_cmd,
        vspipe_args,
        &json_file,
        model,
        res,
        scaler,
        sample_rate,
        filter,
        threads,
        60.0,
        false,
    )?;

    plot_vmaf_score_file(&json_file, &plot_file).unwrap();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_vmaf(
    encoded: &Path,
    reference_pipe_cmd: &[impl AsRef<OsStr>],
    vspipe_args: Vec<String>,
    stat_file: impl AsRef<Path>,
    model: Option<impl AsRef<Path>>,
    res: &str,
    scaler: &str,
    sample_rate: usize,
    vmaf_filter: Option<&str>,
    threads: usize,
    framerate: f64,
    disable_motion: bool,
) -> Result<(), Box<EncoderCrash>> {
    let mut filter = if sample_rate > 1 {
        format!(
            "select=not(mod(n\\,{})),setpts={:.4}*PTS,",
            sample_rate,
            1.0 / sample_rate as f64,
        )
    } else {
        String::new()
    };

    if let Some(vmaf_filter) = vmaf_filter {
        filter.reserve(1 + vmaf_filter.len());
        filter.push_str(vmaf_filter);
        filter.push(',');
    }

    let vmaf = if let Some(model) = model {
        let model_path = if model.as_ref().as_os_str() == "vmaf_v0.6.1neg.json" {
            format!(
                "version=vmaf_v0.6.1neg{}",
                if disable_motion {
                    "\\:motion.motion_force_zero=true"
                } else {
                    ""
                }
            )
        } else {
            format!("path={}", ffmpeg::escape_path_in_filter(&model))
        };
        format!(
            "[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={}:model='{}':\
             n_threads={}",
            ffmpeg::escape_path_in_filter(stat_file),
            model_path,
            threads
        )
    } else {
        format!(
            "[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={}{}:n_threads={}",
            ffmpeg::escape_path_in_filter(stat_file),
            if disable_motion {
                ":model='version=vmaf_v0.6.1\\:motion.motion_force_zero=true'"
            } else {
                ""
            },
            threads
        )
    };

    let mut source_pipe = if let [cmd, args @ ..] = reference_pipe_cmd {
        let mut source_pipe = Command::new(cmd);
        // Append vspipe python arguments to the environment if there are any
        for arg in vspipe_args {
            source_pipe.args(["-a", &arg]);
        }
        source_pipe.args(args);
        source_pipe.stdout(Stdio::piped());
        source_pipe.stderr(Stdio::null());
        source_pipe.spawn().unwrap()
    } else {
        unreachable!()
    };

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-loglevel",
        "error",
        "-hide_banner",
        "-nostdin",
        "-y",
        "-thread_queue_size",
        "1024",
        "-r",
        &framerate.to_string(),
        "-i",
    ]);
    cmd.arg(encoded);
    cmd.args(["-r", &framerate.to_string(), "-i", "-", "-filter_complex"]);

    let distorted = format!(
        "[0:v]scale={}:flags={}:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS,\
         setsar=1[distorted];",
        &res, &scaler
    );
    let reference = format!(
        "[1:v]{}scale={}:flags={}:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS,\
         setsar=1[ref];",
        filter, &res, &scaler
    );

    cmd.arg(format!("{distorted}{reference}{vmaf}"));
    cmd.args(["-f", "null", "-"]);
    cmd.stdin(source_pipe.stdout.take().unwrap());
    cmd.stderr(Stdio::piped());
    cmd.stdout(Stdio::null());

    let output = cmd.output().unwrap();

    if !output.status.success() {
        return Err(Box::new(EncoderCrash {
            exit_status:        output.status,
            source_pipe_stderr: String::new().into(),
            ffmpeg_pipe_stderr: None,
            stderr:             output.stderr.into(),
            stdout:             String::new().into(),
        }));
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_vmaf_weighted(
    encoded: &Path,
    reference_pipe_cmd: &[impl AsRef<OsStr>],
    vspipe_args: Vec<String>,
    stat_file: impl AsRef<Path>,
    model: Option<impl AsRef<Path>>,
    _res: &str,
    _scaler: &str,
    sample_rate: usize,
    vmaf_filter: Option<&str>,
    threads: usize,
    framerate: f64,
    disable_motion: bool,
) -> anyhow::Result<()> {
    let temp_dir = encoded.parent().unwrap();
    let vmaf_y_path = temp_dir.join(format!(
        "vmaf_y_{}.json",
        encoded.file_stem().unwrap().to_str().unwrap()
    ));
    let vmaf_u_path = temp_dir.join(format!(
        "vmaf_u_{}.json",
        encoded.file_stem().unwrap().to_str().unwrap()
    ));
    let vmaf_v_path = temp_dir.join(format!(
        "vmaf_v_{}.json",
        encoded.file_stem().unwrap().to_str().unwrap()
    ));

    let mut filter = if sample_rate > 1 {
        format!(
            "select=not(mod(n\\,{})),setpts={:.4}*PTS,",
            sample_rate,
            1.0 / sample_rate as f64,
        )
    } else {
        String::new()
    };

    if let Some(vmaf_filter) = vmaf_filter {
        filter.reserve(1 + vmaf_filter.len());
        filter.push_str(vmaf_filter);
        filter.push(',');
    }

    let model_str = if let Some(model) = model {
        if model.as_ref().as_os_str() == "vmaf_v0.6.1neg.json" {
            format!(
                "version=vmaf_v0.6.1neg{}",
                if disable_motion {
                    "\\:motion.motion_force_zero=true"
                } else {
                    ""
                }
            )
        } else {
            format!(
                "path={}{}",
                ffmpeg::escape_path_in_filter(&model),
                if disable_motion {
                    "\\:motion.motion_force_zero=true"
                } else {
                    ""
                }
            )
        }
    } else {
        format!(
            "version=vmaf_v0.6.1{}",
            if disable_motion {
                "\\:motion.motion_force_zero=true"
            } else {
                ""
            }
        )
    };

    let mut source_pipe = if let [cmd, args @ ..] = reference_pipe_cmd {
        let mut source_pipe = Command::new(cmd);
        for arg in vspipe_args {
            source_pipe.args(["-a", &arg]);
        }
        source_pipe.args(args);
        source_pipe.stdout(Stdio::piped());
        source_pipe.stderr(Stdio::null());
        source_pipe.spawn().unwrap()
    } else {
        unreachable!()
    };

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-loglevel",
        "error",
        "-hide_banner",
        "-nostdin",
        "-y",
        "-thread_queue_size",
        "1024",
        "-r",
        &framerate.to_string(),
        "-i",
    ]);
    cmd.arg(encoded);
    cmd.args(["-r", &framerate.to_string(), "-i", "-", "-filter_complex"]);

    let filter_complex = format!(
        "[1:v]format=yuv420p[ref];[0:v]format=yuv420p[dis];\
         [dis]extractplanes=y+u+v[dis_y][dis_u][dis_v];\
         [ref]extractplanes=y+u+v[ref_y][ref_u][ref_v];[dis_y][ref_y]libvmaf=log_path={}:\
         log_fmt=json:n_threads={}:n_subsample=1:model='{}':eof_action=endall[vmaf_y_out];\
         [dis_u][ref_u]libvmaf=log_path={}:log_fmt=json:n_threads={}:n_subsample=1:model='{}':\
         eof_action=endall[vmaf_u_out];[dis_v][ref_v]libvmaf=log_path={}:log_fmt=json:\
         n_threads={}:n_subsample=1:model='{}':eof_action=endall[vmaf_v_out]",
        ffmpeg::escape_path_in_filter(&vmaf_y_path),
        threads,
        model_str,
        ffmpeg::escape_path_in_filter(&vmaf_u_path),
        threads,
        model_str,
        ffmpeg::escape_path_in_filter(&vmaf_v_path),
        threads,
        model_str
    );

    cmd.arg(filter_complex);
    cmd.args(["-map", "[vmaf_y_out]", "-map", "[vmaf_u_out]", "-map", "[vmaf_v_out]"]);
    cmd.args(["-an", "-sn", "-dn", "-f", "null", "-"]);
    cmd.stdin(source_pipe.stdout.take().unwrap());
    cmd.stderr(Stdio::piped());
    cmd.stdout(Stdio::null());

    let output = cmd.output().unwrap();

    let _ = source_pipe.wait();

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "FFmpeg command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    if !vmaf_y_path.exists() || !vmaf_u_path.exists() || !vmaf_v_path.exists() {
        return Err(anyhow::anyhow!("Y, U, V VMAF files were not created"));
    }

    let y_scores = read_vmaf_file(&vmaf_y_path).context("Failed to read VMAF Y scores")?;
    let u_scores = read_vmaf_file(&vmaf_u_path).context("Failed to read VMAF U scores")?;
    let v_scores = read_vmaf_file(&vmaf_v_path).context("Failed to read VMAF V scores")?;

    let weighted_scores: Vec<f64> = y_scores
        .iter()
        .zip(u_scores.iter())
        .zip(v_scores.iter())
        .map(|((y, u), v)| (4.0 * y + u + v) / 6.0)
        .collect();

    let weighted_result = VmafResult {
        frames: weighted_scores
            .iter()
            .map(|&score| Metrics {
                metrics: VmafScore {
                    vmaf: score
                },
            })
            .collect(),
    };

    let json_str = serde_json::to_string_pretty(&weighted_result)?;
    std::fs::write(stat_file, json_str)?;

    Ok(())
}

pub fn read_vmaf_file(file: impl AsRef<Path>) -> Result<Vec<f64>, serde_json::Error> {
    let json_str = std::fs::read_to_string(file).unwrap();
    let vmaf_results = serde_json::from_str::<VmafResult>(&json_str)?;
    let v = vmaf_results.frames.into_iter().map(|metric| metric.metrics.vmaf).collect();

    Ok(v)
}

/// Read a certain, given percentile VMAF score from the VMAF json file
pub fn read_weighted_vmaf<P: AsRef<Path>>(
    file: P,
    probe_statistic: ProbingStatistic,
) -> Result<f64, Box<dyn std::error::Error>> {
    let scores = read_vmaf_file(file)?;
    if scores.is_empty() {
        return Err("No VMAF scores found".into());
    }

    // Must be mutable as each computation is cached for reuse in implementation
    let mut metric_statistics = MetricStatistics::new(scores);

    let statistic = match probe_statistic.name {
        ProbingStatisticName::Mean => metric_statistics.mean(),
        ProbingStatisticName::Median => metric_statistics.median(),
        ProbingStatisticName::Harmonic => metric_statistics.harmonic_mean(),
        ProbingStatisticName::Percentile => {
            let value = probe_statistic.value.ok_or("Percentile statistic requires a value")?;
            metric_statistics.percentile(value as usize)
        },
        ProbingStatisticName::StandardDeviation => {
            let value =
                probe_statistic.value.ok_or("Standard deviation statistic requires a value")?;
            let sigma_distance = value * metric_statistics.standard_deviation();
            let statistic = metric_statistics.mean() + sigma_distance;
            statistic.clamp(metric_statistics.minimum(), metric_statistics.maximum())
        },
        ProbingStatisticName::Mode => metric_statistics.mode(),
        ProbingStatisticName::Minimum => metric_statistics.minimum(),
        ProbingStatisticName::Maximum => metric_statistics.maximum(),
        ProbingStatisticName::RootMeanSquare => metric_statistics.root_mean_square(),
    };

    Ok(statistic)
}

pub fn percentile_of_sorted(scores: &[f64], percentile: f64) -> f64 {
    assert!(!scores.is_empty());

    let k = ((scores.len() - 1) as f64 * percentile) as usize;

    scores[k]
}
