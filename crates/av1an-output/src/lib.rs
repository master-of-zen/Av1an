mod concat;
pub use concat::{ffmpeg, ivf, mkvmerge, sort_files_by_filename, ConcatMethod};

#[derive(thiserror::Error, Debug)]
pub enum OutputError {
    #[error("Path error: {0}")]
    Path(#[from] path_abs::Error),

    #[error("Format error: {0}")]
    Format(#[from] std::fmt::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("FFmpeg failed: {0}")]
    FFmpegFailed(String),

    #[error("MKVMerge failed: {0}")]
    MkvMergeFailed(String),

    #[error("IVF concatenation failed: {0}")]
    IvfFailed(String),

    #[error("Pathing Failed: {0}")]
    PathAbsFail(path_abs::Error),
}
