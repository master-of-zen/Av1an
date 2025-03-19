// crates/av1an-output/src/concat.rs
use std::{
    fmt::{Display, Write as FmtWrite},
    fs::{self, DirEntry, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use av_format::{
    buffer::AccReader,
    demuxer::{Context as DemuxerContext, Event},
    muxer::{Context as MuxerContext, Writer},
};
use av_ivf::{demuxer::IvfDemuxer, muxer::IvfMuxer};
use path_abs::{PathAbs, PathInfo};
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use crate::OutputError;

#[derive(
    PartialEq,
    Eq,
    Copy,
    Clone,
    Serialize,
    Deserialize,
    Debug,
    strum::EnumString,
    strum::IntoStaticStr,
)]
pub enum ConcatMethod {
    #[strum(serialize = "mkvmerge")]
    MKVMerge,
    #[strum(serialize = "ffmpeg")]
    FFmpeg,
    #[strum(serialize = "ivf")]
    Ivf,
}

impl Display for ConcatMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(<&'static str>::from(self))
    }
}

pub fn sort_files_by_filename(files: &mut [PathBuf]) {
    files.sort_unstable_by_key(|x| {
        x.file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<u32>()
            .unwrap()
    });
}

pub fn ivf(input: &Path, out: &Path) -> Result<(), OutputError> {
    let mut files: Vec<PathBuf> = fs::read_dir(input)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .collect();

    sort_files_by_filename(&mut files);

    if files.is_empty() {
        return Err(OutputError::IvfFailed("No input files found".into()));
    }

    let output = File::create(out)?;
    let mut muxer = MuxerContext::new(IvfMuxer::new(), Writer::new(output));

    let global_info = {
        let acc = AccReader::new(std::fs::File::open(&files[0])?);
        let mut demuxer = DemuxerContext::new(IvfDemuxer::new(), acc);
        demuxer
            .read_headers()
            .map_err(|e| OutputError::IvfFailed(e.to_string()))?;

        let duration = demuxer.info.duration.unwrap_or(0)
            + files
                .iter()
                .skip(1)
                .filter_map(|file| {
                    let acc =
                        AccReader::new(std::fs::File::open(file).unwrap());
                    let mut demuxer =
                        DemuxerContext::new(IvfDemuxer::new(), acc);
                    demuxer.read_headers().unwrap();
                    demuxer.info.duration
                })
                .sum::<u64>();

        let mut info = demuxer.info;
        info.duration = Some(duration);
        info
    };

    muxer
        .set_global_info(global_info)
        .map_err(|e| OutputError::IvfFailed(e.to_string()))?;
    muxer
        .configure()
        .map_err(|e| OutputError::IvfFailed(e.to_string()))?;
    muxer
        .write_header()
        .map_err(|e| OutputError::IvfFailed(e.to_string()))?;

    let mut pos_offset: usize = 0;
    for file in &files {
        let mut last_pos: usize = 0;
        let input = std::fs::File::open(file)?;
        let acc = AccReader::new(input);
        let mut demuxer = DemuxerContext::new(IvfDemuxer::new(), acc);
        demuxer
            .read_headers()
            .map_err(|e| OutputError::IvfFailed(e.to_string()))?;

        loop {
            match demuxer.read_event() {
                Ok(event) => {
                    match event {
                        Event::MoreDataNeeded(sz) => {
                            return Err(OutputError::IvfFailed(format!(
                                "needed more data: {sz} bytes"
                            )));
                        },
                        Event::NewStream(s) => {
                            return Err(OutputError::IvfFailed(format!(
                                "new stream: {s:?}"
                            )))
                        },
                        Event::NewPacket(mut packet) => {
                            if let Some(p) = packet.pos.as_mut() {
                                last_pos = *p;
                                *p += pos_offset;
                            }
                            muxer.write_packet(Arc::new(packet)).map_err(
                                |e| OutputError::IvfFailed(e.to_string()),
                            )?;
                        },
                        Event::Continue => continue,
                        Event::Eof => break,
                        _ => {
                            return Err(OutputError::IvfFailed(
                                "Unexpected event".into(),
                            ))
                        },
                    }
                },
                Err(e) => {
                    error!("{:?}", e);
                    break;
                },
            }
        }
        pos_offset += last_pos + 1;
    }

    muxer
        .write_trailer()
        .map_err(|e| OutputError::IvfFailed(e.to_string()))?;
    Ok(())
}

pub fn mkvmerge(
    temp_dir: &Path,
    output: &Path,
    encoder_extension: &str,
    num_tasks: usize,
) -> Result<(), OutputError> {
    #[cfg(windows)]
    fn fix_path<P: AsRef<Path>>(p: P) -> String {
        const UNC_PREFIX: &str = r#"\\?\"#;

        let p = p.as_ref().display().to_string();
        if let Some(path) = p.strip_prefix(UNC_PREFIX) {
            if let Some(p2) = path.strip_prefix("UNC") {
                format!("\\{}", p2)
            } else {
                path.to_string()
            }
        } else {
            p
        }
    }

    #[cfg(not(windows))]
    fn fix_path<P: AsRef<Path>>(p: P) -> String {
        p.as_ref().display().to_string()
    }

    let mut audio_file = PathBuf::from(&temp_dir);
    audio_file.push("audio.mkv");
    let audio_file = PathAbs::new(&audio_file)?;
    let audio_file = if audio_file.as_path().exists() {
        Some(fix_path(audio_file))
    } else {
        None
    };

    let mut encode_dir = PathBuf::from(temp_dir);
    encode_dir.push("encode");

    let output = PathAbs::new(output)?;

    assert!(num_tasks != 0);

    let options_path = PathBuf::from(&temp_dir).join("options.json");
    let options_json_contents = mkvmerge_options_json(
        num_tasks,
        encoder_extension,
        &fix_path(output.to_str().unwrap()),
        audio_file.as_deref(),
    );

    let mut options_json = File::create(options_path)?;
    options_json.write_all(options_json_contents.as_bytes())?;

    let mut cmd = Command::new("mkvmerge");
    cmd.current_dir(&encode_dir);
    cmd.arg("@../options.json");

    let out = cmd.output().map_err(|e| OutputError::Io(e))?;

    if !out.status.success() {
        error!(
            "mkvmerge concatenation failed with output: {:#?}\ncommand: {:?}",
            out, cmd
        );
        return Err(OutputError::MkvMergeFailed(
            String::from_utf8_lossy(&out.stderr).into(),
        ));
    }

    Ok(())
}

pub fn mkvmerge_options_json(
    num: usize,
    ext: &str,
    output: &str,
    audio: Option<&str>,
) -> String {
    let mut file_string = String::with_capacity(64 + 12 * num);
    write!(file_string, "[\"-o\", {output:?}").unwrap();
    if let Some(audio) = audio {
        write!(file_string, ", {audio:?}").unwrap();
    }
    file_string.push_str(", \"[\"");
    for i in 0..num {
        write!(file_string, ", \"{i:05}.{ext}\"").unwrap();
    }
    file_string.push_str(",\"]\"]");

    file_string
}

pub fn ffmpeg(temp: &Path, output: &Path) -> Result<(), OutputError> {
    fn write_concat_file(temp_folder: &Path) -> Result<(), OutputError> {
        let concat_file = temp_folder.join("concat");
        let encode_folder = temp_folder.join("encode");

        let mut files: Vec<DirEntry> =
            fs::read_dir(&encode_folder)?.collect::<Result<Vec<_>, _>>()?;

        files.sort_by_key(DirEntry::path);

        let mut contents = String::with_capacity(24 * files.len());

        for i in files {
            writeln!(
                contents,
                "file {}",
                format!("{}", i.path().display())
                    .replace('\\', r"\\")
                    .replace(' ', r"\ ")
                    .replace('\'', r"\'")
            )?;
        }

        let mut file = File::create(concat_file)?;
        file.write_all(contents.as_bytes())?;

        Ok(())
    }

    let temp = PathAbs::new(temp)?;
    let temp = temp.as_path();

    let concat = temp.join("concat");
    let concat_file = concat.to_str().unwrap();

    write_concat_file(temp)?;

    let audio_file = {
        let file = temp.join("audio.mkv");
        if file.exists() && file.metadata().unwrap().len() > 1000 {
            Some(file)
        } else {
            None
        }
    };

    let mut cmd = Command::new("ffmpeg");

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    if let Some(file) = audio_file {
        cmd.args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            concat_file,
            "-i",
        ])
        .arg(file)
        .args(["-map", "0", "-map", "1", "-c", "copy"])
        .arg(output);
    } else {
        cmd.args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            concat_file,
        ])
        .args(["-map", "0", "-c", "copy"])
        .arg(output);
    }

    debug!("FFmpeg concat command: {:?}", cmd);

    let out = cmd.output().map_err(|e| OutputError::Io(e))?;

    if !out.status.success() {
        error!(
            "FFmpeg concatenation failed with output: {:#?}\ncommand: {:?}",
            out, cmd
        );
        return Err(OutputError::FFmpegFailed(
            String::from_utf8_lossy(&out.stderr).into(),
        ));
    }

    Ok(())
}
