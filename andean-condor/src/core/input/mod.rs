//! A decoded video input, and the backends that can produce it.
//!
//! An [`Input`] is decoded natively through FFMS2 or read through VapourSynth.
//! Each has its own filter vocabulary, so building inputs lives in the
//! [`ffms2`] and [`vapoursynth`] submodules, while reading frames back — as
//! y4m or as the raw planes of a [`FrameFeed`] — lives here.

use std::{collections::HashMap, io::Cursor, path::PathBuf, sync::atomic::AtomicBool};

// `::vapoursynth` is the crate, not this module's `vapoursynth` submodule.
use ::vapoursynth::node::Node;
use anyhow::Result;
use av_decoders::{Decoder, VideoDetails, v_frame::chroma::ChromaSubsampling};
pub use av_decoders::{DecoderError, ModifyNode};

pub use crate::core::input::ffms2_filter::{ChromaSampling, Ffms2Filter};
use crate::{
    core::input::clip_info::ClipInfo,
    models::input::{
        ImportMethod,
        Input as InputModel,
        VapourSynthImportMethod,
        VapourSynthScriptSource,
    },
    vapoursynth::vapoursynth_filters::VapourSynthFilter,
};

mod ffms2;
mod frame_feed;
mod vapoursynth;

pub use self::frame_feed::{FrameFeed, RawFrame, RawPlane};

pub mod clip_info;
pub mod color_range;
pub mod ffms2_filter;
pub mod pixel_format;

/// Reports backend indexing progress as `(current, total)` units — a whole
/// percentage for FFMS2, whatever a subprocess's stdout counts for DGDecNV.
pub type IndexProgress<'a> = &'a mut dyn FnMut(u64, u64);

/// The stage of opening an input, as reported by [`Input::ensure_indexed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenProgress {
    /// FFMS2 is indexing the source, `current` percent of `total`. FFMS2
    /// reports whole percentages, so `total` is always `100`.
    Indexing { current: u64, total: u64 },
    /// An index already existed and the input is being opened. No finer
    /// progress is available — VapourSynth indexes inside its source plugin,
    /// out of reach of this process.
    Opening,
}

pub enum Input {
    Video {
        path:           PathBuf,
        import_method:  ImportMethod,
        filters:        Vec<Ffms2Filter>,
        /// The stream's format before `filters` is applied, which is what a
        /// filter's unset fields resolve against. `decoder`'s details describe
        /// the *filtered* output.
        source_details: VideoDetails,
        decoder:        Decoder,
        clip_info:      Option<ClipInfo>,
    },
    VapourSynth {
        path:          PathBuf,
        import_method: VapourSynthImportMethod,
        cache_path:    Option<PathBuf>,
        filters:       Vec<VapourSynthFilter>,
        decoder:       Decoder,
        clip_info:     Option<ClipInfo>,
    },
    VapourSynthScript {
        source:              VapourSynthScriptSource,
        variables:           HashMap<String, String>,
        index:               u8,
        filters:             Vec<VapourSynthFilter>,
        stream_concurrently: bool,
        decoder:             Decoder,
        clip_info:           Option<ClipInfo>,
    },
}

impl Input {
    /// The model this input was built from, for storing and re-opening.
    #[inline]
    pub fn as_data(&self) -> InputModel {
        match self {
            Input::Video {
                path,
                import_method,
                filters,
                ..
            } => InputModel::Video {
                path:          path.clone(),
                import_method: import_method.clone(),
                filters:       filters.clone(),
            },
            Input::VapourSynth {
                path,
                import_method,
                cache_path,
                filters,
                ..
            } => InputModel::VapourSynth {
                path:          path.clone(),
                import_method: import_method.clone(),
                cache_path:    cache_path.clone(),
                filters:       filters.clone(),
            },
            Input::VapourSynthScript {
                source,
                variables,
                index,
                filters,
                stream_concurrently,
                ..
            } => InputModel::VapourSynthScript {
                source:              source.clone(),
                variables:           variables.clone(),
                index:               *index,
                filters:             filters.clone(),
                stream_concurrently: *stream_concurrently,
            },
        }
    }

    /// Checks that `data` names an input this build can open.
    #[inline]
    pub fn validate(data: &InputModel) -> Result<()> {
        match data {
            InputModel::Video {
                path, ..
            } => ffms2::validate(path),
            // TODO: Check if VapourSynth + plugin is installed and static cache it
            InputModel::VapourSynth {
                path, ..
            } => vapoursynth::validate(path),
            InputModel::VapourSynthScript {
                source, ..
            } => vapoursynth::validate_script(source),
        }
    }

    /// Opens the input described by `data`.
    #[inline]
    pub fn from_data(data: &InputModel) -> Result<Self> {
        match data {
            InputModel::Video {
                ..
            } => ffms2::from_video(data),
            InputModel::VapourSynth {
                ..
            }
            | InputModel::VapourSynthScript {
                ..
            } => vapoursynth::from_vapoursynth(data, None),
        }
    }

    /// Opens a native FFMS2 input, applying its filters.
    #[inline]
    pub fn from_video(data: &InputModel) -> Result<Self> {
        ffms2::from_video(data)
    }

    /// Opens a VapourSynth input, chaining `modify_node` and then the input's
    /// own filters onto the source node.
    #[inline]
    pub fn from_vapoursynth(
        data: &InputModel,
        modify_node: Option<av_decoders::ModifyNode>,
    ) -> Result<Self> {
        vapoursynth::from_vapoursynth(data, modify_node)
    }

    /// Opens a VapourSynth input through a generated script that bakes the
    /// input's filters in, for filters `invoke_plugin_function` cannot reach.
    #[inline]
    pub fn from_vapoursynth_scripted(data: &InputModel) -> Result<Self> {
        vapoursynth::from_vapoursynth_scripted(data)
    }

    /// Opens the input described by `data`, reporting indexing progress first.
    ///
    /// A native FFMS2 input is pre-indexed here through av-decoders, so
    /// progress can be reported while the `.ffindex` cache is built. A
    /// VapourSynth input reports a single indeterminate
    /// [`OpenProgress::Opening`]: its source plugin indexes internally, out of
    /// reach of this process. DGDecNV is the one exception, since it shells out
    /// to `dgindexnv` and reports back on its stdout.
    ///
    /// Pass `cancelled` to abort an in-flight index early.
    #[inline]
    pub fn ensure_indexed(
        data: &InputModel,
        mut progress: impl FnMut(OpenProgress),
        cancelled: Option<&AtomicBool>,
    ) -> Result<Self> {
        match data {
            InputModel::Video {
                path,
                import_method,
                ..
            } => {
                // Validated here too: indexing must fail on a missing file
                // with the same error `from_video` would have given.
                Input::validate(data)?;
                let ImportMethod::FFMS2 {
                    cache_path, ..
                } = import_method;
                // If no `.ffindex` exists yet this is the index `from_video`
                // would otherwise build itself; if one does it returns at once
                // and `from_video` reads it straight from the cache.
                ffms2::index_video_with_progress(
                    path,
                    cache_path.as_deref(),
                    Some(&mut |current, total| {
                        progress(OpenProgress::Indexing {
                            current,
                            total,
                        });
                    }),
                    cancelled,
                )?;
                progress(OpenProgress::Opening);
                ffms2::from_video(data)
            },
            InputModel::VapourSynth {
                ..
            } => {
                vapoursynth::index_source(data, &mut progress, cancelled)?;
                progress(OpenProgress::Opening);
                vapoursynth::from_vapoursynth(data, None)
            },
            InputModel::VapourSynthScript {
                ..
            } => {
                progress(OpenProgress::Opening);
                vapoursynth::from_vapoursynth(data, None)
            },
        }
    }

    /// Reopens a natively-decoded input through VapourSynth, translating its
    /// native filters first so both paths convert the clip the same way.
    /// Already-VapourSynth inputs return `None`.
    #[inline]
    pub fn as_vapoursynth_script(&mut self) -> Result<Option<Self>> {
        vapoursynth::as_vapoursynth_script(self)
    }

    /// The input's decoder.
    #[inline]
    pub fn decoder(&mut self) -> &mut Decoder {
        match self {
            Input::Video {
                decoder, ..
            }
            | Input::VapourSynth {
                decoder, ..
            }
            | Input::VapourSynthScript {
                decoder, ..
            } => decoder,
        }
    }

    /// A sequential feed of the raw frames in `[first, end)`, addressed by
    /// absolute index through whichever backend decodes this input. Frames
    /// borrow the feed, so only one decode is resident at a time and no plane
    /// is copied into an av-decoders
    /// [`Frame`](av_decoders::v_frame::frame::Frame) first.
    #[inline]
    pub fn frame_feed(&mut self, first: usize, end: usize) -> Result<FrameFeed<'_>> {
        FrameFeed::new(self, first, end)
    }

    /// Describes the clip, caching the result on the input.
    #[inline]
    pub fn clip_info(&mut self) -> Result<ClipInfo> {
        let slot = match self {
            Input::Video {
                decoder,
                clip_info,
                ..
            } => {
                if clip_info.is_none() {
                    *clip_info = Some(ffms2::clip_info(decoder)?);
                }
                clip_info.as_ref()
            },
            Input::VapourSynth {
                decoder,
                clip_info,
                ..
            }
            | Input::VapourSynthScript {
                decoder,
                clip_info,
                ..
            } => {
                if clip_info.is_none() {
                    *clip_info = Some(vapoursynth::clip_info(decoder)?);
                }
                clip_info.as_ref()
            },
        };
        Ok(*slot.expect("ClipInfo is Some"))
    }

    /// The `C` tag in a y4m header, describing bit depth and chroma layout.
    /// Taken from the decoder rather than `ClipInfo`, so it describes the
    /// frames actually written.
    #[inline]
    pub fn y4m_chroma_tag(bit_depth: usize, chroma: ChromaSubsampling) -> String {
        let chroma_str = match chroma {
            ChromaSubsampling::Monochrome => "mono",
            ChromaSubsampling::Yuv420 => "420",
            ChromaSubsampling::Yuv422 => "422",
            ChromaSubsampling::Yuv444 => "444",
        };
        match chroma {
            // Monochrome has no `p` marker even at high bit depth.
            ChromaSubsampling::Monochrome => chroma_str.to_owned(),
            _ if bit_depth > 8 => format!("{chroma_str}p{bit_depth}"),
            _ => chroma_str.to_owned(),
        }
    }

    /// The y4m stream header for `frames` frames.
    #[inline]
    pub fn y4m_header(&mut self, frames: Option<usize>) -> Result<String> {
        let clip_info = self.clip_info()?;
        let details = self.decoder().get_video_details();

        Ok(format!(
            "YUV4MPEG2 C{} W{} H{} F{}:{} Ip A0:0{}\n",
            Input::y4m_chroma_tag(details.bit_depth, details.chroma_sampling),
            clip_info.resolution.0,
            clip_info.resolution.1,
            clip_info.frame_rate.numer(),
            clip_info.frame_rate.denom(),
            frames.map_or_else(String::new, |frames| format!(" XLENGTH {frames}"))
        ))
    }

    /// Writes frame `index` as a complete y4m frame. Native input packs
    /// FFMS2's planes directly, without building an av-decoders `Frame`.
    #[inline]
    pub fn y4m_frame(&mut self, index: usize) -> Result<Cursor<Vec<u8>>> {
        let mut stream = Cursor::new(Vec::new());
        match self {
            Input::Video {
                decoder, ..
            } => {
                ffms2::y4m_frame(decoder, index, stream.get_mut())?;
            },
            Input::VapourSynth {
                decoder, ..
            }
            | Input::VapourSynthScript {
                decoder, ..
            } => {
                vapoursynth::y4m_frame(decoder, index, stream.get_mut())?;
            },
        }
        Ok(stream)
    }

    /// Sends `frame_indices` as y4m frames through `frame_sender`, in order.
    /// Each frame is sent before the next is decoded, so a bounded
    /// `frame_sender` bounds resident memory. Native input packs FFMS2's own
    /// planes, without building an av-decoders `Frame` per frame.
    #[inline]
    pub fn y4m_frames(
        &mut self,
        frame_sender: crossbeam_channel::Sender<Cursor<Vec<u8>>>,
        frame_indices: &[usize],
    ) -> Result<()> {
        match self {
            Input::Video {
                decoder, ..
            } => {
                let result = ffms2::y4m_frames(decoder, frame_indices, |frame| {
                    frame_sender.send(Cursor::new(frame))?;
                    Ok(())
                });
                drop(frame_sender);
                result?;
            },
            Input::VapourSynth {
                decoder, ..
            }
            | Input::VapourSynthScript {
                decoder, ..
            } => {
                let window = frame_feed::request_window();
                vapoursynth::y4m_frames(
                    &vapoursynth::output_node(decoder)?,
                    &frame_sender,
                    frame_indices,
                    window,
                )?;
                drop(frame_sender);
            },
        }
        Ok(())
    }

    /// Whether [`Input::frame_sources`] can serve several readers at once
    /// without them slowing each other down. False for DGDecNV, which is
    /// unmeasured. A script streams concurrently unless the user turned it
    /// off, since its source plugin is unknown and may be worth measuring.
    #[inline]
    #[must_use]
    pub fn streams_concurrently(&self) -> bool {
        match self {
            Input::Video {
                ..
            } => true,
            Input::VapourSynth {
                import_method, ..
            } => vapoursynth::streams_concurrently(import_method),
            Input::VapourSynthScript {
                stream_concurrently,
                ..
            } => *stream_concurrently,
        }
    }

    /// Creates `count` independent [`FrameSource`]s, so frames can be streamed
    /// on several threads at once. Native sources each open their own
    /// decoder; VapourSynth sources share one plugin instance, so `count`
    /// may exceed the readers an instance can serve.
    #[inline]
    pub fn frame_sources(&mut self, count: usize) -> Result<Vec<FrameSource<'_>>> {
        let data = self.as_data();
        match self {
            Input::Video {
                ..
            } => Ok(std::iter::repeat_with(|| FrameSource {
                kind: FrameSourceKind::Native(data.clone()),
            })
            .take(count)
            .collect()),
            Input::VapourSynth {
                path,
                import_method,
                cache_path,
                decoder,
                ..
            } => vapoursynth::frame_sources(
                count,
                path,
                cache_path.as_deref(),
                import_method,
                decoder,
            )
            .map(|nodes| {
                nodes
                    .into_iter()
                    .map(|node| FrameSource {
                        kind: FrameSourceKind::VapourSynth(node),
                    })
                    .collect()
            }),
            Input::VapourSynthScript {
                decoder, ..
            } => {
                // Every source shares the script's output node.
                let node = vapoursynth::output_node(decoder)?;
                Ok(std::iter::repeat_with(|| FrameSource {
                    kind: FrameSourceKind::VapourSynth(node.clone()),
                })
                .take(count)
                .collect())
            },
        }
    }
}

/// A handle for streaming an [`Input`]'s frames on another thread. Move it to
/// the reading thread and call [`FrameSource::open`] there.
pub struct FrameSource<'a> {
    kind: FrameSourceKind<'a>,
}

enum FrameSourceKind<'a> {
    VapourSynth(Node<'a>),
    /// Native decoders are not thread-safe, so each source opens its own
    /// decoder on the thread that reads from it.
    Native(InputModel),
}

impl<'a> FrameSource<'a> {
    /// Opens the source for reading on the current thread.
    #[inline]
    pub fn open(self) -> Result<FrameReader<'a>> {
        Ok(FrameReader {
            kind: match self.kind {
                FrameSourceKind::VapourSynth(node) => FrameReaderKind::VapourSynth(node),
                FrameSourceKind::Native(data) => {
                    FrameReaderKind::Native(Box::new(Input::from_data(&data)?))
                },
            },
        })
    }
}

/// Reads frames for a [`FrameSource`] on a single thread.
pub struct FrameReader<'a> {
    kind: FrameReaderKind<'a>,
}

enum FrameReaderKind<'a> {
    VapourSynth(Node<'a>),
    Native(Box<Input>),
}

impl FrameReader<'_> {
    /// Sends `frame_indices` as Y4M frames through `frame_sender`, in order.
    /// VapourSynth sources keep at most `window` frames in flight, so with a
    /// bounded `frame_sender` resident frames stay bounded by `window`.
    #[inline]
    pub fn y4m_frames(
        &mut self,
        frame_sender: &crossbeam_channel::Sender<Cursor<Vec<u8>>>,
        frame_indices: &[usize],
        window: usize,
    ) -> Result<()> {
        match &mut self.kind {
            FrameReaderKind::VapourSynth(node) => {
                vapoursynth::y4m_frames(node, frame_sender, frame_indices, window.max(1))
            },
            FrameReaderKind::Native(input) => input.y4m_frames(frame_sender.clone(), frame_indices),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("No video file found at {0}")]
    VideoFileNotFound(PathBuf),
    #[error("No VapourSynth script file found at {0}")]
    VapourSynthScriptNotFound(PathBuf),
    #[error("File {0} is not a video file")]
    NotAVideoFile(PathBuf),
    #[error("File {0} is not a VapourSynth script")]
    NotAVapourSynthScript(PathBuf),
}
#[cfg(test)]
mod tests {
    use av_decoders::Rational32;

    use super::*;
    use crate::{
        core::input::{
            ffms2_filter::{ChromaSampling, ffms2_pixel_format},
            pixel_format::PixelFormat,
        },
        ffmpeg::FFPixelFormat,
        vapoursynth::plugins::resize::Scaler,
    };

    /// `VideoDetails::default` only exists under av-decoders' own `cfg(test)`.
    fn video_details(
        width: usize,
        height: usize,
        bit_depth: usize,
        chroma_sampling: ChromaSubsampling,
    ) -> VideoDetails {
        VideoDetails {
            width,
            height,
            bit_depth,
            chroma_sampling,
            frame_rate: Rational32::new(24, 1),
            total_frames: Some(100),
            color_range: None,
            color_primaries: None,
            color_transfer_characteristic: None,
            color_space: None,
            chroma_location: None,
            mastering_display: None,
            content_light_level: None,
        }
    }

    /// A mismatch here hands the encoder mis-parsed frames, since `y4m_frames`
    /// picks the sample type from `ClipInfo`.
    #[test]
    fn chroma_tag_agrees_with_clip_info_bit_depth() {
        for (bit_depth, chroma) in [
            (8, ChromaSubsampling::Yuv420),
            (10, ChromaSubsampling::Yuv420),
            (12, ChromaSubsampling::Yuv420),
            (8, ChromaSubsampling::Yuv444),
            (10, ChromaSubsampling::Yuv444),
            (8, ChromaSubsampling::Monochrome),
            (10, ChromaSubsampling::Monochrome),
        ] {
            let tag = Input::y4m_chroma_tag(bit_depth, chroma);
            let format =
                ffms2_pixel_format(bit_depth, chroma).expect("FFMS2 should support this format");
            let info = PixelFormat::FFmpeg {
                format,
            };

            assert_eq!(
                info.as_bit_depth().expect("bit depth should resolve"),
                bit_depth,
                "tag {tag} reports a different depth than ClipInfo"
            );
        }
    }

    #[test]
    fn chroma_tag_matches_y4m_convention() {
        assert_eq!(Input::y4m_chroma_tag(8, ChromaSubsampling::Yuv420), "420");
        assert_eq!(
            Input::y4m_chroma_tag(10, ChromaSubsampling::Yuv420),
            "420p10"
        );
        assert_eq!(
            Input::y4m_chroma_tag(12, ChromaSubsampling::Yuv422),
            "422p12"
        );
        assert_eq!(
            Input::y4m_chroma_tag(10, ChromaSubsampling::Yuv444),
            "444p10"
        );
        // Monochrome never takes the `p` marker.
        assert_eq!(
            Input::y4m_chroma_tag(10, ChromaSubsampling::Monochrome),
            "mono"
        );
    }

    #[test]
    fn unsupported_output_format_is_rejected() {
        // FFMS2 has no 16-bit output, so the mapping must not invent one.
        assert!(ffms2_pixel_format(16, ChromaSubsampling::Yuv420).is_none());
        assert!(ffms2_pixel_format(10, ChromaSubsampling::Yuv420).is_some());
    }

    /// A native filter must become a VapourSynth resize, so a VS-only path sees
    /// the same converted clip the native path does.
    #[test]
    fn native_filter_becomes_vapoursynth_resize() {
        let baseline = video_details(1920, 1080, 8, ChromaSubsampling::Yuv420);

        let ten_bit = Ffms2Filter::OutputFormat {
            bit_depth: Some(10),
            chroma:    None,
            width:     None,
            height:    None,
        };
        assert_eq!(ten_bit.to_vapoursynth_filters(&baseline), vec![
            VapourSynthFilter::Resize {
                scaler: Some(Scaler::Bicubic),
                width:  None,
                height: None,
                format: Some(FFPixelFormat::YUV420P10LE),
            }
        ]);

        let downscale = Ffms2Filter::OutputFormat {
            bit_depth: None,
            chroma:    None,
            width:     Some(1280),
            height:    Some(720),
        };
        assert_eq!(downscale.to_vapoursynth_filters(&baseline), vec![
            VapourSynthFilter::Resize {
                scaler: Some(Scaler::Bicubic),
                width:  Some(1280),
                height: Some(720),
                format: None,
            }
        ]);
    }

    /// A filter matching the decoded stream is a no-op and must not add work.
    #[test]
    fn native_filter_matching_baseline_is_dropped() {
        let baseline = video_details(1920, 1080, 10, ChromaSubsampling::Yuv420);

        let noop = Ffms2Filter::OutputFormat {
            bit_depth: Some(10),
            chroma:    Some(ChromaSampling::Yuv420),
            width:     Some(1920),
            height:    Some(1080),
        };
        assert!(noop.to_vapoursynth_filters(&baseline).is_empty());

        let empty = Ffms2Filter::OutputFormat {
            bit_depth: None,
            chroma:    None,
            width:     None,
            height:    None,
        };
        assert!(empty.to_vapoursynth_filters(&baseline).is_empty());
    }

    /// `set_output_format` rewrites the decoder's details, so the filtered
    /// format equals each filter's resolved result. Resolving against it
    /// would make every filter a no-op and drop them from the VapourSynth
    /// script.
    #[test]
    fn filters_resolve_against_the_source_not_the_filtered_format() {
        let source = video_details(1920, 1080, 8, ChromaSubsampling::Yuv420);

        let filters = [
            Ffms2Filter::OutputFormat {
                bit_depth: Some(10),
                chroma:    Some(ChromaSampling::Yuv444),
                width:     None,
                height:    None,
            },
            // Unset fields inherit from the previous filter, as they do when the
            // decoder applies them in sequence.
            Ffms2Filter::OutputFormat {
                bit_depth: None,
                chroma:    None,
                width:     Some(1280),
                height:    Some(720),
            },
        ];

        // Stand in for the decoder's details after the filters were applied.
        let mut current = source;
        for filter in &filters {
            let Ffms2Filter::OutputFormat {
                bit_depth,
                chroma,
                width,
                height,
            } = filter;
            let bit_depth = bit_depth.map_or(current.bit_depth, usize::from);
            let chroma = chroma.map_or(current.chroma_sampling, |chroma| {
                chroma.to_chroma_subsampling()
            });
            current = VideoDetails {
                width: width.map_or(current.width, |width| width as usize),
                height: height.map_or(current.height, |height| height as usize),
                bit_depth,
                chroma_sampling: chroma,
                ..current
            };
        }
        assert_eq!(
            (
                current.width,
                current.height,
                current.bit_depth,
                current.chroma_sampling
            ),
            (1280, 720, 10, ChromaSubsampling::Yuv444)
        );

        // Against the filtered format every filter looks like a no-op...
        for filter in &filters {
            assert!(
                filter.to_vapoursynth_filters(&current).is_empty(),
                "a filter should not be dropped just because the decoder already applied it"
            );
        }

        // ...whereas against the source it resolves to a real conversion.
        let converted = filters
            .iter()
            .flat_map(|filter| filter.to_vapoursynth_filters(&source))
            .collect::<Vec<_>>();
        assert_eq!(converted, vec![
            VapourSynthFilter::Resize {
                scaler: Some(Scaler::Bicubic),
                width:  None,
                height: None,
                format: Some(FFPixelFormat::YUV444P10LE),
            },
            VapourSynthFilter::Resize {
                scaler: Some(Scaler::Bicubic),
                width:  Some(1280),
                height: Some(720),
                format: None,
            },
        ]);
    }
}
