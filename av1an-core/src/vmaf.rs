use std::{
    cmp::Ordering,
    ffi::OsStr,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::anyhow;
use plotters::prelude::*;
use serde::Deserialize;

use crate::{
    broker::EncoderCrash,
    ffmpeg,
};

#[derive(Deserialize, Debug)]
struct VmafScore {
    vmaf: f64,
}

#[derive(Deserialize, Debug)]
struct Metrics {
    metrics: VmafScore,
}

#[derive(Deserialize, Debug)]
struct VmafResult {
    frames: Vec<Metrics>,
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
            "FFmpeg is not compiled with --enable-libvmaf, but target quality \
             or VMAF plotting was enabled"
        ));
    }
    Ok(())
}

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
        format!(
            "[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:\
             log_path={}:model='path={}':n_threads={}",
            ffmpeg::escape_path_in_filter(stat_file),
            ffmpeg::escape_path_in_filter(&model),
            threads
        )
    } else {
        format!(
            "[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:\
             log_path={}:n_threads={}",
            ffmpeg::escape_path_in_filter(stat_file),
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
        "-y",
        "-thread_queue_size",
        "1024",
        "-hide_banner",
        "-r",
        "60",
        "-i",
    ]);
    cmd.arg(encoded);
    cmd.args(["-r", "60", "-i", "-", "-filter_complex"]);

    let distorted = format!(
        "[0:v]scale={}:flags={}:force_original_aspect_ratio=decrease,\
         setpts=PTS-STARTPTS,setsar=1[distorted];",
        &res, &scaler
    );
    let reference = format!(
        "[1:v]{}scale={}:flags={}:force_original_aspect_ratio=decrease,\
         setpts=PTS-STARTPTS,setsar=1[ref];",
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

pub fn read_vmaf_file(
    file: impl AsRef<Path>,
) -> Result<Vec<f64>, serde_json::Error> {
    let json_str = std::fs::read_to_string(file).unwrap();
    let vmaf_results = serde_json::from_str::<VmafResult>(&json_str)?;
    let v = vmaf_results
        .frames
        .into_iter()
        .map(|metric| metric.metrics.vmaf)
        .collect();

    Ok(v)
}

/// Read a certain percentile VMAF score from the VMAF json file
///
/// Do not call this function more than once on the same json file,
/// as this function is only more efficient for a single read.
pub fn read_weighted_vmaf<P: AsRef<Path>>(
    file: P,
    percentile: f64,
) -> Result<f64, serde_json::Error> {
    fn inner(file: &Path, percentile: f64) -> Result<f64, serde_json::Error> {
        let mut scores = read_vmaf_file(file)?;

        assert!(!scores.is_empty());

        let k = ((scores.len() - 1) as f64 * percentile) as usize;

        // if we are just calling this function a single time for this file, it
        // is more efficient to use select_nth_unstable_by than it is to
        // completely sort scores
        let (_, kth_element, _) = scores.select_nth_unstable_by(k, |a, b| {
            a.partial_cmp(b).unwrap_or(Ordering::Less)
        });

        Ok(*kth_element)
    }

    inner(file.as_ref(), percentile)
}

/// Calculates percentile from an array of sorted values
pub fn percentile_of_sorted(scores: &[f64], percentile: f64) -> f64 {
    assert!(!scores.is_empty());

    let k = ((scores.len() - 1) as f64 * percentile) as usize;

    scores[k]
}
