use std::{
    cmp::Ordering,
    ffi::OsStr,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::anyhow;

use crate::{broker::EncoderCrash, ffmpeg};

pub fn validate_libxpsnr() -> anyhow::Result<()> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-filters");

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let out = cmd.output()?;

    let stdr = String::from_utf8(out.stdout)?;
    if !stdr.contains("xpsnr") {
        return Err(anyhow!(
            "FFmpeg is not compiled with XPSNR or is outdated, but target quality or XPSNR \
             plotting was enabled"
        ));
    }
    Ok(())
}

pub fn run_xpsnr(
    encoded: &Path,
    reference_pipe_cmd: &[impl AsRef<OsStr>],
    vspipe_args: Vec<String>,
    stat_file: impl AsRef<Path>,
    res: &str,
    scaler: &str,
    sample_rate: usize,
) -> Result<(), Box<EncoderCrash>> {
    let filter = if sample_rate > 1 {
        format!(
            "select=not(mod(n\\,{})),setpts={:.4}*PTS,",
            sample_rate,
            1.0 / sample_rate as f64,
        )
    } else {
        String::new()
    };

    let xpsnr = format!(
        "[distorted][ref]xpsnr=stats_file={}:eof_action=endall",
        ffmpeg::escape_path_in_filter(stat_file)
    );

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
        "[0:v]scale={}:flags={}:force_original_aspect_ratio=decrease,setsar=1[distorted];",
        &res, &scaler
    );
    let reference = format!(
        "[1:v]{filter}scale={}:flags={}:force_original_aspect_ratio=decrease,setsar=1[ref];",
        &res, &scaler
    );

    cmd.arg(format!("{distorted}{reference}{xpsnr}"));
    cmd.args(["-f", "null", "-"]);
    cmd.stdin(source_pipe.stdout.take().unwrap());
    cmd.stderr(Stdio::piped());
    cmd.stdout(Stdio::null());

    let output = cmd.output().unwrap();

    if !output.status.success() {
        println!(
            "FFmpeg exited with status {}",
            String::from_utf8(output.stdout.clone()).unwrap()
        );
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

pub fn read_xpsnr_file(file: impl AsRef<Path>) -> Vec<f64> {
    let log_str = std::fs::read_to_string(file).unwrap();
    let re = regex::Regex::new(
        r".*XPSNR y: *([0-9\.]+|inf) *XPSNR u: *([0-9\.]+|inf) *XPSNR v: *([0-9\.]+|inf)",
    )
    .unwrap();
    let mut min_psnrs = Vec::new();
    for line in log_str.lines() {
        if let Some(captures) = re.captures(line) {
            let min_psnr = captures
                .iter()
                .skip(1)
                .filter_map(|x| {
                    x.unwrap().as_str().parse::<f64>().ok().or_else(|| {
                        if x.unwrap().as_str() == "inf" {
                            Some(f64::INFINITY)
                        } else {
                            panic!("XPSNR line did not contain a valid float or 'inf'!")
                        }
                    })
                })
                .fold(f64::INFINITY, f64::min);
            min_psnrs.push(min_psnr);
        }
    }

    min_psnrs
}

// Read a certain percentile XPSNR score from the PSNR log file
pub fn read_weighted_xpsnr<P: AsRef<Path>>(
    file: P,
    percentile: f64,
) -> Result<f64, serde_json::Error> {
    fn inner(file: &Path, percentile: f64) -> Result<f64, serde_json::Error> {
        let mut scores = read_xpsnr_file(file);

        assert!(!scores.is_empty());

        let k = ((scores.len() - 1) as f64 * percentile) as usize;

        // if we are just calling this function a single time for this file, it is more
        // efficient to use select_nth_unstable_by than it is to completely sort
        // scores
        let (_, kth_element, _) =
            scores.select_nth_unstable_by(k, |a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));

        Ok(*kth_element)
    }

    inner(file.as_ref(), percentile)
}
