//! Configuration for fmetrics scoring: which metric, and how to read the input.
//!
//! Colour properties come from the caller's [`VideoDetails`] rather than being
//! hardcoded, because the YUV to RGB conversion in [`crate::convert`] is part
//! of the measurement: a wrong matrix silently changes every score.

use av_decoders::VideoDetails;

use crate::error::FmetricsError;

/// The default worker count, matching the VapourSynth plugin path's four
/// handlers so the two agree when `threads` is unset.
pub const DEFAULT_THREADS: u32 = 4;

/// The p-norm Butteraugli reduces by when none is configured: the L3 norm the
/// metric is defined against, and also what the library reads `pnorm <= 0` as.
pub const DEFAULT_PNORM: i32 = 3;

/// A metric fmetrics can compute.
///
/// IW-SSIM and MS-SSIM operate over five scales and score in the opposite
/// direction to the perceptual metrics, which is why
/// [`FmetricsMetric::prefers_lower_is_better`] exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FmetricsMetric {
    /// SSIMULACRA2.
    Ssimulacra2,
    /// Butteraugli.
    Butteraugli,
    /// CVVDP.
    Cvvdp,
    /// IW-SSIM, a scale-invariant structural similarity.
    Iwssim,
    /// MS-SSIM, multi-scale structural similarity.
    Msssim,
}

impl FmetricsMetric {
    /// A stable lowercase name, for logs and error messages.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ssimulacra2 => "ssimu2",
            Self::Butteraugli => "butteraugli",
            Self::Cvvdp => "cvvdp",
            Self::Iwssim => "iwssim",
            Self::Msssim => "msssim",
        }
    }

    /// Whether a lower score is better.
    ///
    /// Butteraugli is a distance; the rest are quality measures. Pooling a
    /// metric the wrong way round silently inverts a quality judgement.
    #[inline]
    #[must_use]
    pub const fn prefers_lower_is_better(self) -> bool {
        matches!(self, Self::Butteraugli)
    }

    /// The minimum image dimension the metric can score, or [`None`] if it has
    /// no floor.
    ///
    /// The two multi-scale metrics need an image reducible four times, and they
    /// do not agree on where that begins: IW-SSIM refuses a 15-pixel image with
    /// [`crate::FmetricsErr::IwssimImgTooSmall`] where MS-SSIM still scores it.
    /// The larger floor is given so a caller has one number to check before
    /// decoding; the library decides the exact one.
    #[inline]
    #[must_use]
    pub const fn minimum_dimension(self) -> Option<usize> {
        match self {
            Self::Iwssim | Self::Msssim => Some(16),
            Self::Ssimulacra2 | Self::Butteraugli | Self::Cvvdp => None,
        }
    }

    /// Whether the metric needs a frame rate.
    ///
    /// Only CVVDP, whose temporal filter is parameterised by the rate.
    #[inline]
    #[must_use]
    pub const fn is_temporal(self) -> bool {
        matches!(self, Self::Cvvdp)
    }
}

/// The YUV to RGB matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YuvMatrix {
    /// ITU-R BT.601.
    Bt601,
    /// ITU-R BT.709.
    Bt709,
    /// ITU-R BT.2020 non-constant luminance.
    Bt2020,
}

/// Luma or chroma sample range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleRange {
    /// Limited (studio) range: luma 16-235, chroma 16-240.
    Limited,
    /// Full range: 0-255 throughout.
    Full,
}

/// The colour properties fmetrics needs to read a frame.
///
/// fmetrics understands only sRGB and linear sRGB, so the *conversion* matrix
/// and range are ours to supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorInfo {
    /// The YUV to RGB matrix.
    pub matrix:    YuvMatrix,
    /// Sample range.
    pub range:     SampleRange,
    /// Source bit depth.
    pub bit_depth: usize,
    /// Whether the source carries HDR values.
    ///
    /// fmetrics has no PQ or HLG transfer function on its input side, but it
    /// does accept an `hdr` flag and the matching HDR display model. PQ/HLG
    /// samples pass through unmodified rather than being linearised here.
    ///
    /// [`av_decoders`] reports no transfer function, so this is set by the
    /// caller rather than detected.
    pub hdr:       bool,
}

impl ColorInfo {
    /// Derive the colour properties from a clip's details.
    ///
    /// `av-decoders` reports no colour metadata, so the matrix and range are
    /// the same declared values libvship is given on the GPU path, which
    /// keeps the two engines comparable. Both are public fields, so a
    /// caller that knows the real characteristics can override them.
    ///
    /// # Errors
    ///
    /// Returns an error for a bit depth fmetrics cannot represent. 8-bit maps
    /// to `RGB_UINT8`; 10- and 12-bit are up-converted to `RGB_UINT16`
    /// rather than truncated.
    #[inline]
    pub fn from_details(details: &VideoDetails) -> Result<Self, FmetricsError> {
        if !matches!(details.bit_depth, 8 | 10 | 12) {
            return Err(FmetricsError::UnsupportedFormat {
                reason: format!(
                    "{}-bit input cannot be represented: fmetrics accepts 8-bit directly and 10- \
                     or 12-bit up-converted to 16-bit",
                    details.bit_depth
                ),
            });
        }

        Ok(Self {
            matrix:    matrix_for_resolution(details.height),
            // Declared rather than guessed: unflagged SD and HD is limited, and
            // assuming full would lift the shadows of every clip.
            range:     SampleRange::Limited,
            bit_depth: details.bit_depth,
            hdr:       false,
        })
    }

    /// Mark this source as carrying HDR values.
    #[inline]
    #[must_use]
    pub const fn with_hdr(mut self, hdr: bool) -> Self {
        self.hdr = hdr;
        self
    }

    /// Override the declared matrix.
    #[inline]
    #[must_use]
    pub const fn with_matrix(mut self, matrix: YuvMatrix) -> Self {
        self.matrix = matrix;
        self
    }

    /// Override the declared range.
    #[inline]
    #[must_use]
    pub const fn with_range(mut self, range: SampleRange) -> Self {
        self.range = range;
        self
    }

    /// The peak sample value for this bit depth, which is what the conversion
    /// normalises against.
    ///
    /// # Errors
    ///
    /// Returns an error for a bit depth with no defined peak.
    #[inline]
    pub fn max_sample(&self) -> Result<u32, FmetricsError> {
        match self.bit_depth {
            8 => Ok(255),
            10 => Ok(1023),
            12 => Ok(4095),
            // Reaching this arm means `ColorInfo` was built by hand with an
            // unsupported depth.
            depth => Err(FmetricsError::UnsupportedFormat {
                reason: format!("no peak sample value is defined for {depth}-bit"),
            }),
        }
    }

    /// Whether samples must be handed over as 16-bit.
    ///
    /// Anything above 8 bits is up-converted rather than truncated.
    #[inline]
    #[must_use]
    pub const fn needs_wide_samples(&self) -> bool {
        self.bit_depth > 8
    }
}

/// The YUV matrix to assume for a picture of this height.
///
/// The 650-pixel threshold is the one libvship's own FFmpeg plugin uses, so the
/// same clip is declared identically on both engine paths.
#[inline]
#[must_use]
pub const fn matrix_for_resolution(height: usize) -> YuvMatrix {
    if height > 650 {
        YuvMatrix::Bt709
    } else {
        YuvMatrix::Bt601
    }
}

/// The fmetrics colorspace a clip should be submitted as.
///
/// fmetrics accepts only sRGB and linear sRGB, so every source is submitted as
/// sRGB; a PQ or HLG source additionally sets `hdr`, and the display model
/// selects the matching HDR characteristics.
#[inline]
#[must_use]
pub const fn colorspace_from_details(details: &VideoDetails) -> crate::ffi::FmetricsColorspace {
    let _ = details;
    crate::ffi::FmetricsColorspace::Srgb
}

/// Configuration for a scoring run.
#[derive(Debug, Clone)]
pub struct FmetricsConfig {
    /// The metric to compute.
    pub metric:                 FmetricsMetric,
    /// Worker count for the per-pair metrics.
    pub threads:                u32,
    /// CVVDP display model.
    pub display_model:          crate::ffi::FmetricsCvvdpDisplayModel,
    /// CVVDP frame rate, in frames per second.
    ///
    /// Required for CVVDP: its temporal filter is parameterised by the rate,
    /// and a rate of zero accumulates error silently rather than failing.
    pub frame_rate:             f32,
    /// Butteraugli p-norm exponent.
    pub butteraugli_pnorm:      Option<i32>,
    /// Butteraugli intensity target.
    pub intensity_target:       f32,
    /// Whether a temporal context's history is cleared at a discontinuity.
    ///
    /// The C API has no `disableTemporal` argument, only
    /// `fmetrics_cvvdp_reset`, so this is the closest it can express. Not a
    /// substitute for disabling the temporal filter: a caller wanting each
    /// frame scored independently reads each pair as it is returned rather
    /// than pooling.
    pub reset_on_discontinuity: bool,
}

impl FmetricsConfig {
    /// A configuration for `metric` with default options.
    #[inline]
    #[must_use]
    pub const fn new(metric: FmetricsMetric) -> Self {
        Self {
            metric,
            threads: DEFAULT_THREADS,
            display_model: crate::ffi::FmetricsCvvdpDisplayModel::StandardFhd,
            frame_rate: 0.0,
            butteraugli_pnorm: None,
            intensity_target: 203.0,
            reset_on_discontinuity: true,
        }
    }

    /// Set whether a temporal context's history is cleared at a discontinuity.
    #[inline]
    #[must_use]
    pub const fn with_reset_on_discontinuity(mut self, reset_on_discontinuity: bool) -> Self {
        self.reset_on_discontinuity = reset_on_discontinuity;
        self
    }

    /// Whether temporal history is carried across a discontinuity, in the
    /// spelling a VapourSynth plugin uses for the opposite behaviour.
    ///
    /// The inverse of [`Self::with_reset_on_discontinuity`]: upstream
    /// `disableTemporal` clears the context, so setting it means *not*
    /// resetting.
    #[inline]
    #[must_use]
    pub const fn with_disable_temporal(self, disable_temporal: bool) -> Self {
        self.with_reset_on_discontinuity(!disable_temporal)
    }

    /// Set the Butteraugli intensity target. The default of 203 is the
    /// reference intensity the metric is defined against.
    #[inline]
    #[must_use]
    pub const fn with_intensity_target(mut self, intensity_target: f32) -> Self {
        self.intensity_target = intensity_target;
        self
    }

    /// Set the worker count.
    #[inline]
    #[must_use]
    pub const fn with_threads(mut self, threads: u32) -> Self {
        self.threads = threads;
        self
    }

    /// Set the CVVDP frame rate.
    #[inline]
    #[must_use]
    pub const fn with_frame_rate(mut self, frame_rate: f32) -> Self {
        self.frame_rate = frame_rate;
        self
    }

    /// Set the CVVDP display model.
    #[inline]
    #[must_use]
    pub const fn with_display_model(
        mut self,
        display_model: crate::ffi::FmetricsCvvdpDisplayModel,
    ) -> Self {
        self.display_model = display_model;
        self
    }

    /// Set the Butteraugli p-norm exponent.
    #[inline]
    #[must_use]
    pub const fn with_pnorm(mut self, pnorm: i32) -> Self {
        self.butteraugli_pnorm = Some(pnorm);
        self
    }

    /// The configured worker count, before [`Self::effective_threads`] clamps
    /// it.
    #[inline]
    #[must_use]
    pub const fn threads(&self) -> u32 {
        self.threads
    }

    /// The metric being computed.
    #[inline]
    #[must_use]
    pub const fn metric(&self) -> FmetricsMetric {
        self.metric
    }

    /// The CVVDP frame rate, where zero means none was configured.
    #[inline]
    #[must_use]
    pub const fn frame_rate(&self) -> f32 {
        self.frame_rate
    }

    /// The CVVDP display model.
    #[inline]
    #[must_use]
    pub const fn display_model(&self) -> crate::ffi::FmetricsCvvdpDisplayModel {
        self.display_model
    }

    /// The effective worker count, with zero clamped to one.
    ///
    /// Zero means "all cores" to the library, which the scorer resolves itself;
    /// a pool of no workers is not a meaningful request.
    #[inline]
    #[must_use]
    pub const fn effective_threads(&self) -> u32 {
        if self.threads == 0 { 1 } else { self.threads }
    }

    /// Whether the configured metric is temporal and needs a frame rate.
    #[inline]
    #[must_use]
    pub const fn is_temporal(&self) -> bool {
        self.metric.is_temporal()
    }

    /// The Butteraugli options this configuration implies.
    ///
    /// A configured p-norm passes through; an absent one becomes
    /// [`DEFAULT_PNORM`], which is also what the library reads `pnorm <= 0` as.
    ///
    /// There is no infinity-norm sentinel: the library computes `pow(d,
    /// pnorm)`, so a large exponent overflows and returns non-finite
    /// scores.
    #[inline]
    #[must_use]
    pub const fn butteraugli_options(&self) -> crate::ffi::FmetricsButteraugliOptions {
        crate::ffi::FmetricsButteraugliOptions {
            intensity_target: self.intensity_target,
            pnorm:            match self.butteraugli_pnorm {
                Some(pnorm) => pnorm,
                None => DEFAULT_PNORM,
            },
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "a failing assertion should panic loudly, which is what a unit test wants"
)]
mod tests {
    use av_decoders::Rational32;
    use v_frame::chroma::ChromaSubsampling;

    use super::*;

    /// Video details in the shape `av-decoders` reports them.
    fn details(width: usize, height: usize, bit_depth: usize) -> VideoDetails {
        VideoDetails {
            width,
            height,
            bit_depth,
            chroma_sampling: ChromaSubsampling::Yuv420,
            frame_rate: Rational32::new(24, 1),
            total_frames: None,
            color_range: None,
            color_primaries: None,
            color_transfer_characteristic: None,
            color_space: None,
            chroma_location: None,
            mastering_display: None,
            content_light_level: None,
        }
    }

    #[test]
    fn the_default_pnorm_stays_inside_the_range_the_library_can_compute() {
        // The library computes `pow(d, pnorm)` and reads `pnorm <= 0` as 3.0.
        // An earlier default of `i32::MAX` -- intended as an "infinity norm" --
        // overflowed that exponentiation and returned non-finite scores on every
        // frame. The infinity norm is simply not expressible through this API.
        let options = FmetricsConfig::new(FmetricsMetric::Butteraugli).butteraugli_options();
        assert_eq!(options.pnorm, DEFAULT_PNORM);
        assert!(
            (1..=64).contains(&options.pnorm),
            "the default exponent must be one the library can evaluate, got {}",
            options.pnorm
        );

        // A configured norm is still passed through verbatim.
        let chosen = FmetricsConfig::new(FmetricsMetric::Butteraugli)
            .with_pnorm(2)
            .butteraugli_options();
        assert_eq!(chosen.pnorm, 2);
    }

    #[test]
    fn matrix_follows_the_same_height_threshold_as_the_gpu_path() {
        assert_eq!(matrix_for_resolution(1080), YuvMatrix::Bt709);
        assert_eq!(matrix_for_resolution(651), YuvMatrix::Bt709);
        assert_eq!(matrix_for_resolution(650), YuvMatrix::Bt601);
        assert_eq!(matrix_for_resolution(480), YuvMatrix::Bt601);
    }

    #[test]
    fn color_info_matches_what_the_gpu_path_declares() {
        // Both engines must declare the same clip identically, or their scores
        // differ for reasons that have nothing to do with the metric.
        let hd = ColorInfo::from_details(&details(1920, 1080, 8)).unwrap();
        assert_eq!(hd.matrix, YuvMatrix::Bt709);
        assert_eq!(hd.range, SampleRange::Limited);

        let sd = ColorInfo::from_details(&details(720, 480, 8)).unwrap();
        assert_eq!(sd.matrix, YuvMatrix::Bt601);
    }

    #[test]
    fn a_caller_can_override_what_av_decoders_cannot_report() {
        // av-decoders exposes no colour metadata, so a caller that knows its
        // content has to be able to say so.
        let info = ColorInfo::from_details(&details(1920, 1080, 10))
            .unwrap()
            .with_matrix(YuvMatrix::Bt2020)
            .with_range(SampleRange::Full)
            .with_hdr(true);

        assert_eq!(info.matrix, YuvMatrix::Bt2020);
        assert_eq!(info.range, SampleRange::Full);
        assert!(info.hdr);
    }

    #[test]
    fn peak_sample_values_match_the_bit_depths() {
        for (depth, peak) in [(8usize, 255u32), (10, 1023), (12, 4095)] {
            let info = ColorInfo::from_details(&details(1920, 1080, depth)).unwrap();
            assert_eq!(info.max_sample().unwrap(), peak, "{depth}-bit");
        }
    }

    #[test]
    fn eight_bit_needs_narrow_samples_and_higher_needs_wide() {
        let eight = ColorInfo::from_details(&details(1920, 1080, 8)).unwrap();
        assert!(!eight.needs_wide_samples());

        for depth in [10, 12] {
            let wide = ColorInfo::from_details(&details(1920, 1080, depth)).unwrap();
            assert!(wide.needs_wide_samples(), "{depth}-bit must up-convert");
        }
    }

    #[test]
    fn unsupported_bit_depth_is_rejected_rather_than_rescaled() {
        for depth in [9, 11, 14, 16] {
            assert!(
                matches!(
                    ColorInfo::from_details(&details(1920, 1080, depth)),
                    Err(FmetricsError::UnsupportedFormat { .. })
                ),
                "{depth}-bit must be rejected"
            );
        }
    }

    #[test]
    fn absent_norm_selects_the_library_default() {
        // Previously `i32::MAX`, on the theory that an infinity norm needs a large
        // exponent. The library evaluates `pow(d, pnorm)`, so that overflowed and
        // returned non-finite scores for every frame -- this test asserted the
        // sentinel without ever checking that the library could compute it.
        let config = FmetricsConfig::new(FmetricsMetric::Butteraugli);
        assert_eq!(config.butteraugli_options().pnorm, DEFAULT_PNORM);
    }

    #[test]
    fn a_configured_norm_is_passed_through() {
        for pnorm in [2, 3, 5] {
            let config = FmetricsConfig::new(FmetricsMetric::Butteraugli).with_pnorm(pnorm);
            assert_eq!(config.butteraugli_options().pnorm, pnorm);
        }
    }

    #[test]
    fn zero_threads_is_clamped_to_one() {
        let config = FmetricsConfig::new(FmetricsMetric::Ssimulacra2).with_threads(0);
        assert_eq!(config.effective_threads(), 1);
    }

    #[test]
    fn default_threads_matches_the_plugin_path() {
        assert_eq!(DEFAULT_THREADS, 4);
        assert_eq!(FmetricsConfig::new(FmetricsMetric::Ssimulacra2).threads, 4);
    }

    #[test]
    fn only_cvvdp_is_temporal() {
        assert!(FmetricsConfig::new(FmetricsMetric::Cvvdp).is_temporal());
        assert!(!FmetricsConfig::new(FmetricsMetric::Ssimulacra2).is_temporal());
        assert!(!FmetricsConfig::new(FmetricsMetric::Butteraugli).is_temporal());
    }

    #[test]
    fn cvvdp_defaults_to_the_fhd_display_model() {
        // This matches the GPU path's default, so an unconfigured CVVDP pass
        // compares against the same target range.
        assert_eq!(
            FmetricsConfig::new(FmetricsMetric::Cvvdp).display_model,
            crate::ffi::FmetricsCvvdpDisplayModel::StandardFhd
        );
    }
}
