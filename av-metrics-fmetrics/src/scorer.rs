//! Streaming scoring through fmetrics.
//!
//! The frame-independent metrics -- SSIMULACRA2, Butteraugli, IW-SSIM and
//! MS-SSIM -- score one pair per call, reusing a workspace for scratch. A
//! pair's score is final the moment the call returns.
//!
//! CVVDP accumulates, so one context sees every frame in order. The caller
//! detects a discontinuity in the selection and calls
//! [`FmetricsScorer::reset_temporal`]; whether a gap is a scene break is the
//! caller's knowledge, not the scorer's.
//!
//! # Sharing
//!
//! A per-pair scorer is [`Send`] and [`Sync`]. fmetrics' scratch arena is
//! per-workspace, so distinct workspaces are independent and concurrent
//! submissions overlap -- but sharing *one* workspace between two threads
//! corrupts it silently, so the pool hands each to one caller at a time.
//!
//! A temporal scorer is [`Send`] but not [`Sync`], since its frames must pass
//! through one accumulator in order.

use std::{
    ffi::c_int,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    config::{ColorInfo, FmetricsConfig, FmetricsMetric},
    convert::{PlaneSet, RgbImage, RgbSource, RgbView, yuv_to_rgb},
    error::FmetricsError,
    ffi::{self, FmetricsApi, FmetricsCvvdpCtx, FmetricsCvvdpResult, FmetricsWorkspace},
};

/// A score for one frame pair.
///
/// Only the field for the configured metric is populated, so a caller can tell
/// which metric produced a score without being told separately.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameScore {
    /// SSIMULACRA2, higher is better.
    pub ssimulacra2:   Option<f64>,
    /// Butteraugli, a distance where lower is better.
    pub butteraugli:   Option<f64>,
    /// CVVDP's JOD, the headline measure, where higher is better.
    pub cvvdp_jod:     Option<f64>,
    /// CVVDP's quality score, which fmetrics reports alongside JOD.
    pub cvvdp_quality: Option<f64>,
    /// IW-SSIM, higher is better.
    pub iwssim:        Option<f64>,
    /// MS-SSIM, higher is better.
    pub msssim:        Option<f64>,
}

impl FrameScore {
    /// A score holding only `value`, for `metric`.
    #[inline]
    #[must_use]
    pub const fn with(metric: FmetricsMetric, value: f64) -> Self {
        Self {
            ssimulacra2:   matches_if(metric, FmetricsMetric::Ssimulacra2, value),
            butteraugli:   matches_if(metric, FmetricsMetric::Butteraugli, value),
            cvvdp_jod:     matches_if(metric, FmetricsMetric::Cvvdp, value),
            cvvdp_quality: None,
            iwssim:        matches_if(metric, FmetricsMetric::Iwssim, value),
            msssim:        matches_if(metric, FmetricsMetric::Msssim, value),
        }
    }

    /// A score with every field empty, for a metric whose result has more than
    /// one part.
    #[inline]
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ssimulacra2:   None,
            butteraugli:   None,
            cvvdp_jod:     None,
            cvvdp_quality: None,
            iwssim:        None,
            msssim:        None,
        }
    }

    /// The value for `metric`, or [`None`] if this score is for another one.
    #[inline]
    #[must_use]
    pub const fn value(&self, metric: FmetricsMetric) -> Option<f64> {
        match metric {
            FmetricsMetric::Ssimulacra2 => self.ssimulacra2,
            FmetricsMetric::Butteraugli => self.butteraugli,
            FmetricsMetric::Cvvdp => self.cvvdp_jod,
            FmetricsMetric::Iwssim => self.iwssim,
            FmetricsMetric::Msssim => self.msssim,
        }
    }

    /// The value for `metric`, or an error naming what this score holds.
    ///
    /// # Errors
    ///
    /// Returns an error if the score was produced for a different metric, which
    /// is a programming error at the call site rather than a runtime condition.
    #[inline]
    pub fn require(&self, metric: FmetricsMetric) -> Result<f64, FmetricsError> {
        self.value(metric).ok_or_else(|| FmetricsError::MetricUnavailable {
            metric: metric.as_str().to_owned(),
            reason: "the score was produced for a different metric".to_owned(),
        })
    }
}

/// `Some(value)` when `metric` is `wanted`, so a score can be built without a
/// chain of branches.
#[inline]
#[must_use]
const fn matches_if(metric: FmetricsMetric, wanted: FmetricsMetric, value: f64) -> Option<f64> {
    let equal = match metric {
        FmetricsMetric::Ssimulacra2 => matches!(wanted, FmetricsMetric::Ssimulacra2),
        FmetricsMetric::Butteraugli => matches!(wanted, FmetricsMetric::Butteraugli),
        FmetricsMetric::Cvvdp => matches!(wanted, FmetricsMetric::Cvvdp),
        FmetricsMetric::Iwssim => matches!(wanted, FmetricsMetric::Iwssim),
        FmetricsMetric::Msssim => matches!(wanted, FmetricsMetric::Msssim),
    };
    if equal { Some(value) } else { None }
}

/// A reusable scratch workspace.
///
/// The library keeps a bump arena behind this handle, so holding one across
/// frames avoids reallocating it per frame.
#[derive(Debug)]
struct Workspace {
    /// The library's handle.
    handle: FmetricsWorkspace,
    /// The next free index, or [`WorkspacePool::NONE`]. A free-list link, not
    /// workspace state.
    next:   AtomicUsize,
}

impl Workspace {
    /// Create a workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the library returns a null handle.
    #[inline]
    fn new(api: &'static FmetricsApi) -> Result<Self, FmetricsError> {
        // SAFETY: `workspace_create` takes no arguments and returns a handle the
        // library guarantees to be non-null on success.
        let handle = unsafe { (api.workspace_create)() };
        if handle.is_null() {
            return Err(FmetricsError::CallFailed {
                function: "fmetrics_workspace_create",
                code:     0,
                message:  "returned a null workspace".to_owned(),
            });
        }
        Ok(Self {
            handle,
            next: AtomicUsize::new(WorkspacePool::NONE),
        })
    }
}

impl Drop for Workspace {
    #[inline]
    fn drop(&mut self) {
        // A load failure means the library was never resolved, so there is
        // nothing to destroy.
        if let Ok(api) = FmetricsApi::load() {
            // SAFETY: the handle came from `workspace_create` and is destroyed
            // once, here.
            unsafe { (api.workspace_destroy)(self.handle) };
        }
    }
}

/// The workspaces, handed out as a free list.
///
/// The invariant is that a workspace is never in two hands at once, because
/// sharing one corrupts fmetrics' scratch silently rather than failing. A "next
/// index" counter cannot guarantee it -- two threads more than `len` apart are
/// handed the same handle. A free list does: the head names only workspaces
/// nobody is using, so a claim is exclusive by construction.
///
/// The nodes live in the pool, so claiming neither allocates nor blocks.
#[derive(Debug)]
struct WorkspacePool {
    /// One workspace per worker, each holding the index of the next free node.
    workspaces: Vec<Workspace>,
    /// The head of the free list, or [`WorkspacePool::NONE`] when all are in
    /// use.
    free:       AtomicUsize,
}

impl WorkspacePool {
    /// The sentinel meaning "no workspace is free".
    const NONE: usize = usize::MAX;

    /// Take a workspace for exclusive use, or report that all are in use.
    ///
    /// The caller must return it with [`WorkspacePool::release`] on every path.
    /// `None` rather than blocking: an oversubscribed caller is told so rather
    /// than made to wait behind work it cannot see.
    fn claim(&self) -> Option<FmetricsWorkspace> {
        let mut head = self.free.load(Ordering::Acquire);
        loop {
            if head == Self::NONE {
                return None;
            }
            let next = self.workspaces[head].next.load(Ordering::Relaxed);
            match self.free.compare_exchange_weak(head, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(self.workspaces[head].handle),
                // The head moved under us; re-read it and try the new one.
                Err(observed) => head = observed,
            }
        }
    }

    /// Return a workspace, putting it back at the head of the free list.
    ///
    /// # Panics
    ///
    /// Panics if `workspace` is not a handle from this pool, since mapping a
    /// handle back to its list node is what keeps the list well formed.
    fn release(&self, workspace: FmetricsWorkspace) {
        let index = self
            .workspaces
            .iter()
            .position(|candidate| candidate.handle == workspace)
            .unwrap_or_else(|| panic!("released a workspace that is not in this pool"));

        // Pushed, so the workspace just used is the next one claimed and its
        // scratch stays hot in cache.
        self.workspaces[index]
            .next
            .store(self.free.swap(index, Ordering::AcqRel), Ordering::Relaxed);
    }

    /// How many workspaces are held.
    #[inline]
    #[must_use]
    fn len(&self) -> usize {
        self.workspaces.len()
    }
}

/// Scores frame pairs through fmetrics.
///
/// A per-pair metric keeps one workspace per worker; CVVDP keeps a single
/// context instead, because its frames must be seen in order by one
/// accumulator.
pub struct FmetricsScorer {
    /// The configuration this scorer was built from.
    config:     FmetricsConfig,
    /// The colour properties both sides are read with.
    color:      ColorInfo,
    /// The peak sample value for the source depth.
    peak:       u32,
    /// The colorspace both images are submitted as.
    colorspace: ffi::FmetricsColorspace,
    /// The per-pair workspaces or the CVVDP context.
    inner:      Inner,
}

enum Inner {
    /// Workspaces for a frame-independent metric, behind a free list so several
    /// threads can be inside the library at once.
    PerPair {
        /// The workspaces.
        pool: WorkspacePool,
    },
    /// The single context a temporal metric accumulates into.
    Temporal {
        /// The CVVDP context, or null until the first frame supplies a
        /// geometry.
        context: FmetricsCvvdpCtx,
        /// Width the context was created for.
        width:   c_int,
        /// Height the context was created for.
        height:  c_int,
    },
}

// SAFETY: both handles are `Send + Sync` newtypes this crate never
// dereferences, so the scorer is already both automatically. What is *not*
// automatic is that a temporal scorer's frames stay in order -- that is
// enforced by `submit_pair_temporal` taking `&mut self`, not by the auto
// traits.
unsafe impl Sync for FmetricsScorer {
}

impl std::fmt::Debug for FmetricsScorer {
    #[inline]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FmetricsScorer")
            .field("metric", &self.config.metric)
            .field("bit_depth", &self.color.bit_depth)
            .field("hdr", &self.color.hdr)
            .field("worker_count", &self.worker_count())
            .finish_non_exhaustive()
    }
}

impl FmetricsScorer {
    /// Build a scorer for `config`, reading frames as `color` describes.
    ///
    /// # Errors
    ///
    /// Returns an error if fmetrics is unavailable, if the bit depth has no
    /// peak sample value, or if CVVDP is configured without a usable frame
    /// rate.
    #[inline]
    pub fn new(config: FmetricsConfig, color: ColorInfo) -> Result<Self, FmetricsError> {
        let api = FmetricsApi::load()?;
        let peak = color.max_sample()?;

        if config.is_temporal() {
            Self::validate_frame_rate(&config)?;
        }

        let inner = if config.is_temporal() {
            Inner::Temporal {
                context: FmetricsCvvdpCtx(std::ptr::null_mut()),
                width:   0,
                height:  0,
            }
        } else {
            // One workspace per thread that may submit at once, so a caller never
            // finds the pool exhausted by its own concurrency.
            let count = config.effective_threads() as usize;
            let mut pool = Vec::with_capacity(count);
            for _ in 0..count {
                pool.push(Workspace::new(api)?);
            }

            // Thread the free list through the workspaces in index order.
            for (index, workspace) in pool.iter_mut().enumerate() {
                let successor = index + 1;
                workspace.next.store(
                    if successor < count {
                        successor
                    } else {
                        WorkspacePool::NONE
                    },
                    Ordering::Relaxed,
                );
            }

            Inner::PerPair {
                pool: WorkspacePool {
                    workspaces: pool,
                    free:       AtomicUsize::new(0),
                },
            }
        };

        Ok(Self {
            config,
            color,
            peak,
            // Depends only on the clip, which `ColorInfo` already records.
            colorspace: ffi::FmetricsColorspace::Srgb,
            inner,
        })
    }

    /// Reject a CVVDP configuration with no usable frame rate.
    ///
    /// A zero rate accumulates error silently rather than failing, so it must
    /// be refused up front rather than producing a plausible-looking wrong
    /// number.
    fn validate_frame_rate(config: &FmetricsConfig) -> Result<(), FmetricsError> {
        if config.frame_rate.is_finite() && config.frame_rate > 0.0 {
            Ok(())
        } else {
            Err(FmetricsError::InvalidConfiguration {
                reason: format!(
                    "CVVDP needs the source frame rate, and {} is not one; a zero rate \
                     accumulates error silently rather than failing",
                    config.frame_rate
                ),
            })
        }
    }

    /// The configuration this scorer was built from.
    #[inline]
    #[must_use]
    pub const fn config(&self) -> &FmetricsConfig {
        &self.config
    }

    /// The metric being computed.
    #[inline]
    #[must_use]
    pub const fn metric(&self) -> FmetricsMetric {
        self.config.metric
    }

    /// The number of per-pair workers.
    ///
    /// A temporal metric always reports one, because its frames must pass
    /// through a single accumulator in order.
    #[inline]
    #[must_use]
    pub fn worker_count(&self) -> usize {
        match &self.inner {
            Inner::PerPair {
                pool,
            } => pool.len().max(1),
            Inner::Temporal {
                ..
            } => 1,
        }
    }

    /// Whether fmetrics is available on this system.
    #[inline]
    #[must_use]
    pub fn is_available() -> bool {
        FmetricsApi::load().is_ok()
    }

    /// The fmetrics version, if it is loaded.
    #[inline]
    #[must_use]
    pub fn fmetrics_version() -> Option<String> {
        FmetricsApi::load().ok().map(FmetricsApi::version_string)
    }

    /// Score one pair that is already interleaved RGB.
    ///
    /// The same measurement as [`FmetricsScorer::submit_pair`] with the
    /// conversion done by the caller, for a caller that already holds RGB.
    ///
    /// The pixel format must match the source depth: the library's metric is
    /// depth-dependent, so reinterpreting bytes would change the score.
    ///
    /// # Errors
    ///
    /// Returns an error if the dimensions differ, if the frames are too small
    /// for the metric, or if fmetrics rejects the pair.
    #[inline]
    pub fn submit_rgb(
        &self,
        reference: &RgbView<'_>,
        distorted: &RgbView<'_>,
    ) -> Result<FrameScore, FmetricsError> {
        if let Some(minimum) = self.config.metric.minimum_dimension()
            && reference.width().min(reference.height()) < minimum
        {
            return Err(FmetricsError::UnsupportedFormat {
                reason: format!(
                    "{} needs at least {minimum} pixels in each dimension to score five scales, \
                     but the frame is {}x{}",
                    self.config.metric.as_str(),
                    reference.width(),
                    reference.height()
                ),
            });
        }

        let Inner::PerPair {
            pool,
        } = &self.inner
        else {
            return Err(FmetricsError::MetricUnavailable {
                metric: self.config.metric.as_str().to_owned(),
                reason: "a temporal metric needs `submit_pair_temporal`, which preserves frame \
                         order"
                    .to_owned(),
            });
        };

        Self::check_dimensions(reference, distorted)?;

        self.with_workspace(pool, |workspace| {
            self.score_per_pair(workspace, reference, distorted, self.colorspace)
        })
    }

    /// Run `score` with a workspace held exclusively, returning it afterwards.
    ///
    /// The release lives here rather than at each call site so an early return
    /// or a `?` cannot leak a workspace and starve the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if every workspace is in use, meaning the caller is
    /// submitting from more threads at once than the pool has workspaces.
    #[inline]
    fn with_workspace<T>(
        &self,
        pool: &WorkspacePool,
        score: impl FnOnce(FmetricsWorkspace) -> Result<T, FmetricsError>,
    ) -> Result<T, FmetricsError> {
        let Some(workspace) = pool.claim() else {
            return Err(FmetricsError::PoolExhausted {
                workspaces: pool.len(),
                reason:     "every workspace is in use; the scorer needs one workspace per thread \
                             that submits at once"
                    .to_owned(),
            });
        };

        // The result is bound before the release so the workspace goes back even
        // if `score` fails.
        let result = score(workspace);
        pool.release(workspace);
        result
    }

    /// Score one pair of planar frames.
    ///
    /// Takes `&self` and is safe to call from several threads; each call claims
    /// its own workspace, so the library calls overlap rather than serialising.
    /// A temporal metric needs [`FmetricsScorer::submit_pair_temporal`]
    /// instead, because one context accumulates across frames and its order
    /// is part of the measurement.
    ///
    /// # Errors
    ///
    /// Returns an error if the frames cannot be converted, or if fmetrics
    /// rejects the pair.
    #[inline]
    pub fn submit_pair(
        &self,
        reference: &PlaneSet,
        distorted: &PlaneSet,
    ) -> Result<FrameScore, FmetricsError> {
        Self::check_size(self, reference)?;

        // SAFETY: both plane sets come from the decode driver, which keeps each
        // frame's planes alive for the submission, and the conversion reads
        // within the allocation each plane describes.
        let reference_image = unsafe { yuv_to_rgb(reference, &self.color, self.peak)? };
        // SAFETY: as above.
        let distorted_image = unsafe { yuv_to_rgb(distorted, &self.color, self.peak)? };

        Self::check_dimensions(&reference_image, &distorted_image)?;

        let Inner::PerPair {
            pool,
        } = &self.inner
        else {
            return Err(FmetricsError::MetricUnavailable {
                metric: self.config.metric.as_str().to_owned(),
                reason: "a temporal metric needs `submit_pair_temporal`, which preserves frame \
                         order"
                    .to_owned(),
            });
        };

        self.with_workspace(pool, |workspace| {
            self.score_per_pair(
                workspace,
                &reference_image,
                &distorted_image,
                self.colorspace,
            )
        })
    }

    /// Reject a frame too small for the configured metric.
    fn check_size(&self, reference: &PlaneSet) -> Result<(), FmetricsError> {
        // Reported here rather than left to the library, whose error code does
        // not name the cause.
        if let Some(minimum) = self.config.metric.minimum_dimension()
            && reference[0].width.min(reference[0].height) < minimum
        {
            return Err(FmetricsError::UnsupportedFormat {
                reason: format!(
                    "{} needs at least {minimum} pixels in each dimension to score five scales, \
                     but the frame is {}x{}",
                    self.config.metric.as_str(),
                    reference[0].width,
                    reference[0].height
                ),
            });
        }
        Ok(())
    }

    /// Reject a pair whose two sides differ in size.
    fn check_dimensions<R: RgbSource>(reference: &R, distorted: &R) -> Result<(), FmetricsError> {
        if reference.width() != distorted.width() || reference.height() != distorted.height() {
            return Err(FmetricsError::InputMismatch {
                reason: format!(
                    "reference is {}x{} but distorted is {}x{}",
                    reference.width(),
                    reference.height(),
                    distorted.width(),
                    distorted.height()
                ),
            });
        }
        Ok(())
    }

    /// Score one pair with a temporal metric, which must see frames in order.
    ///
    /// # Errors
    ///
    /// Returns an error if the frames cannot be converted, or if fmetrics
    /// rejects the pair.
    #[inline]
    pub fn submit_pair_temporal(
        &mut self,
        reference: &PlaneSet,
        distorted: &PlaneSet,
    ) -> Result<FrameScore, FmetricsError> {
        Self::check_size(self, reference)?;

        // SAFETY: as in `submit_pair`.
        let reference_image = unsafe { yuv_to_rgb(reference, &self.color, self.peak)? };
        // SAFETY: as above.
        let distorted_image = unsafe { yuv_to_rgb(distorted, &self.color, self.peak)? };

        Self::check_dimensions(&reference_image, &distorted_image)?;

        let Inner::Temporal {
            context,
            width,
            height,
        } = &mut self.inner
        else {
            return Err(FmetricsError::MetricUnavailable {
                metric: self.config.metric.as_str().to_owned(),
                reason: "a frame-independent metric must be scored through a workspace".to_owned(),
            });
        };
        let mut context = *context;
        let (mut current_width, mut current_height) = (*width, *height);

        let result = self.score_cvvdp(
            &mut context,
            &mut current_width,
            &mut current_height,
            &reference_image,
            &distorted_image,
            self.colorspace,
        );
        // The context is created lazily on the first frame, so its state is
        // written back.
        if let Inner::Temporal {
            context: slot,
            width,
            height,
        } = &mut self.inner
        {
            *slot = context;
            *width = current_width;
            *height = current_height;
        }

        result
    }

    /// Score one pair with the frame-independent metric.
    fn score_per_pair<R: RgbSource>(
        &self,
        workspace: FmetricsWorkspace,
        reference: &R,
        distorted: &R,
        colorspace: ffi::FmetricsColorspace,
    ) -> Result<FrameScore, FmetricsError> {
        let api = FmetricsApi::load()?;
        let reference_img = reference.as_img(colorspace, self.color.hdr);
        let distorted_img = distorted.as_img(colorspace, self.color.hdr);

        let mut value = 0f64;
        let metric = self.config.metric;

        // The function called depends only on the metric; every signature takes the
        // same workspace and image pair.
        let (function, status) = match metric {
            FmetricsMetric::Ssimulacra2 => {
                // SAFETY: the pointers match the signatures transcribed from
                // `fmetrics.h`; the workspace is live, and both images outlive
                // the call because the caller owns them.
                ("fmetrics_ssimu2_cmp", unsafe {
                    (api.ssimu2_cmp)(workspace, &reference_img, &distorted_img, &mut value)
                })
            },
            FmetricsMetric::Butteraugli => {
                let options = self.config.butteraugli_options();
                // SAFETY: as above; `options` outlives the call.
                ("fmetrics_butteraugli_cmp", unsafe {
                    (api.butteraugli_cmp)(
                        workspace,
                        &reference_img,
                        &distorted_img,
                        &options,
                        &mut value,
                    )
                })
            },
            FmetricsMetric::Iwssim => (
                "fmetrics_iwssim_cmp",
                // SAFETY: as above.
                unsafe { (api.iwssim_cmp)(workspace, &reference_img, &distorted_img, &mut value) },
            ),
            FmetricsMetric::Msssim => (
                "fmetrics_msssim_cmp",
                // SAFETY: as above.
                unsafe { (api.msssim_cmp)(workspace, &reference_img, &distorted_img, &mut value) },
            ),
            // Unreachable: a CVVDP scorer holds `Inner::Temporal`.
            FmetricsMetric::Cvvdp => {
                return Err(FmetricsError::MetricUnavailable {
                    metric: metric.as_str().to_owned(),
                    reason: "a temporal metric cannot be scored through a workspace".to_owned(),
                });
            },
        };

        ffi::check(status, function)?;
        Ok(FrameScore::with(metric, value))
    }

    /// Score one pair through the CVVDP context, creating it if this is the
    /// first frame or the geometry has changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the context cannot be created, or if fmetrics
    /// rejects the frame.
    fn score_cvvdp(
        &self,
        context: &mut FmetricsCvvdpCtx,
        width: &mut c_int,
        height: &mut c_int,
        reference: &RgbImage,
        distorted: &RgbImage,
        colorspace: ffi::FmetricsColorspace,
    ) -> Result<FrameScore, FmetricsError> {
        let api = FmetricsApi::load()?;
        let reference_img = reference.as_img(colorspace, self.color.hdr);
        let distorted_img = distorted.as_img(colorspace, self.color.hdr);

        let frame_width = reference_img.width as c_int;
        let frame_height = reference_img.height as c_int;

        // The context is bound to a geometry, so a change of size means recreating it.
        if (*context).is_null() || *width != frame_width || *height != frame_height {
            self.recreate_cvvdp(context, frame_width, frame_height)?;
            *width = frame_width;
            *height = frame_height;
        }

        let mut result = FmetricsCvvdpResult::default();
        // SAFETY: `*context` is a live context created by `cvvdp_create` below or
        // by an earlier frame, and both images outlive the call.
        let status = unsafe {
            (api.cvvdp_process_frame)(*context, &reference_img, &distorted_img, &mut result)
        };
        ffi::check(status, "fmetrics_cvvdp_process_frame")?;

        let mut score = FrameScore::empty();
        score.cvvdp_jod = Some(result.jod);
        score.cvvdp_quality = Some(result.quality);
        Ok(score)
    }

    /// Destroy any existing context and create one for this geometry.
    ///
    /// # Errors
    ///
    /// Returns an error if fmetrics refuses to create the context.
    fn recreate_cvvdp(
        &self,
        context: &mut FmetricsCvvdpCtx,
        width: c_int,
        height: c_int,
    ) -> Result<(), FmetricsError> {
        let api = FmetricsApi::load()?;

        if !(*context).is_null() {
            // SAFETY: the context came from `cvvdp_create` and is released once.
            unsafe { (api.cvvdp_destroy)(*context) };
            *context = FmetricsCvvdpCtx(std::ptr::null_mut());
        }

        let mut created = FmetricsCvvdpCtx(std::ptr::null_mut());
        // SAFETY: the arguments match the signature. A null `custom_params`
        // selects the display model's own parameters.
        let status = unsafe {
            (api.cvvdp_create)(
                width,
                height,
                self.config.frame_rate,
                self.config.display_model,
                self.config.effective_threads(),
                std::ptr::null(),
                &mut created,
            )
        };
        ffi::check(status, "fmetrics_cvvdp_create")?;

        if created.is_null() {
            return Err(FmetricsError::CallFailed {
                function: "fmetrics_cvvdp_create",
                code:     0,
                message:  "returned a null context".to_owned(),
            });
        }

        *context = created;
        Ok(())
    }

    /// Clear the temporal history, for a scene break.
    ///
    /// A per-pair metric has no history to clear, so this succeeds without
    /// calling the library.
    ///
    /// # Errors
    ///
    /// Returns an error if fmetrics rejects the reset.
    #[inline]
    pub fn reset_temporal(&mut self) -> Result<(), FmetricsError> {
        let Inner::Temporal {
            context, ..
        } = &mut self.inner
        else {
            return Ok(());
        };

        // Nothing has been scored yet, so there is no context and no history.
        if context.is_null() {
            return Ok(());
        }

        let api = FmetricsApi::load()?;
        // SAFETY: the context is live and was created by `cvvdp_create`.
        let status = unsafe { (api.cvvdp_reset)(*context) };
        ffi::check(status, "fmetrics_cvvdp_reset")
    }
}

impl Drop for FmetricsScorer {
    #[inline]
    fn drop(&mut self) {
        if let Inner::Temporal {
            context, ..
        } = self.inner
            && !context.is_null()
            && let Ok(api) = FmetricsApi::load()
        {
            // SAFETY: the context was created by `cvvdp_create` and is destroyed
            // exactly once, here.
            unsafe { (api.cvvdp_destroy)(context) };
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "a failing assertion should panic loudly, which is what a unit test wants"
)]
mod tests {
    use av_decoders::{Rational32, VideoDetails};
    use v_frame::chroma::ChromaSubsampling;

    use super::*;
    use crate::convert::PlaneSource;

    fn details(bit_depth: usize) -> VideoDetails {
        VideoDetails {
            width: 64,
            height: 64,
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

    /// A neutral 4:4:4 frame, leaked so its pointer outlives the test.
    fn plane(width: usize, height: usize, samples: Vec<u8>) -> PlaneSource {
        let leaked = Box::leak(samples.into_boxed_slice());
        PlaneSource {
            width,
            height,
            stride: width as isize,
            data_origin: 0,
            data: leaked.as_ptr(),
            bytes_per_sample: 1,
        }
    }

    fn frame(width: usize, height: usize, fill: u8) -> PlaneSet {
        let size = width * height;
        PlaneSet {
            planes: [
                plane(width, height, vec![fill; size]),
                plane(width, height, vec![128u8; size]),
                plane(width, height, vec![128u8; size]),
            ],
        }
    }

    fn color(bit_depth: usize) -> ColorInfo {
        ColorInfo::from_details(&details(bit_depth)).unwrap()
    }

    #[test]
    fn a_scorer_needs_the_library() {
        // Without fmetrics this must be a clean error, not a panic: the caller
        // falls back to the VapourSynth plugin when this tier is unavailable.
        let config = FmetricsConfig::new(FmetricsMetric::Ssimulacra2);
        match FmetricsScorer::new(config, color(8)) {
            Ok(_) => assert!(FmetricsScorer::is_available()),
            Err(error) => assert!(
                matches!(error, FmetricsError::LibraryNotFound { .. }),
                "expected unavailability, got {error:?}"
            ),
        }
    }

    #[test]
    fn a_temporal_metric_needs_the_temporal_entry_point() {
        // A CVVDP scorer holds a single accumulating context, so the per-pair
        // entry point must refuse it rather than silently scoring each frame as
        // if it were independent. This is the failure a caller hits when it
        // submits a temporal metric through `submit_pair`: the error names the
        // right remedy, which is what makes it diagnosable at the call site.
        let Ok(mut scorer) = FmetricsScorer::new(
            FmetricsConfig::new(FmetricsMetric::Cvvdp).with_frame_rate(24.0),
            color(8),
        ) else {
            return;
        };

        let size = 64;
        let planes = frame(size, size, 100);

        let error = scorer
            .submit_pair(&planes, &planes)
            .expect_err("submit_pair must refuse a temporal metric");
        assert!(
            matches!(error, FmetricsError::MetricUnavailable { .. }),
            "expected an unavailability error naming the remedy, got {error:?}"
        );
        assert!(
            error.to_string().contains("submit_pair_temporal"),
            "the error should name the entry point that does work, got {error}"
        );

        // The temporal entry point is the one that accepts it, which is what
        // makes the error above a routing mistake rather than a dead end.
        assert!(
            scorer.submit_pair_temporal(&planes, &planes).is_ok(),
            "submit_pair_temporal must accept a temporal metric"
        );
    }

    #[test]
    fn cvvdp_rejects_a_missing_frame_rate() {
        // A zero rate accumulates error silently, so it must be refused up front
        // rather than producing a plausible-looking but wrong number. The check
        // runs before the library is consulted, so it holds either way.
        let config = FmetricsConfig::new(FmetricsMetric::Cvvdp);
        assert!(
            FmetricsScorer::validate_frame_rate(&config).is_err(),
            "a zero rate must be refused"
        );

        // Through the constructor, the error may instead be unavailability if the
        // library is absent -- but never a successful construction.
        if let Err(error) = FmetricsScorer::new(config, color(8)) {
            assert!(
                matches!(
                    error,
                    FmetricsError::InvalidConfiguration { .. }
                        | FmetricsError::LibraryNotFound { .. }
                ),
                "expected a configuration or availability error, got {error:?}"
            );
        }
    }

    #[test]
    fn frame_rate_validation_rejects_non_finite_and_zero() {
        for rate in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
            let config = FmetricsConfig::new(FmetricsMetric::Cvvdp).with_frame_rate(rate);
            assert!(
                FmetricsScorer::validate_frame_rate(&config).is_err(),
                "{rate} must be rejected"
            );
        }
        let good = FmetricsConfig::new(FmetricsMetric::Cvvdp).with_frame_rate(24.0);
        assert!(FmetricsScorer::validate_frame_rate(&good).is_ok());
    }

    #[test]
    fn frame_score_reports_only_the_metric_it_was_asked_for() {
        let score = FrameScore::with(FmetricsMetric::Ssimulacra2, 0.9);
        assert_eq!(score.value(FmetricsMetric::Ssimulacra2), Some(0.9));
        for other in [
            FmetricsMetric::Butteraugli,
            FmetricsMetric::Cvvdp,
            FmetricsMetric::Iwssim,
            FmetricsMetric::Msssim,
        ] {
            assert_eq!(score.value(other), None, "{other:?} must be unset");
            assert!(score.require(other).is_err());
        }
    }

    #[test]
    fn every_metric_has_a_name_and_a_direction() {
        let metrics = [
            FmetricsMetric::Ssimulacra2,
            FmetricsMetric::Butteraugli,
            FmetricsMetric::Cvvdp,
            FmetricsMetric::Iwssim,
            FmetricsMetric::Msssim,
        ];
        for metric in metrics {
            assert!(!metric.as_str().is_empty(), "{metric:?} needs a name");
        }

        // Butteraugli is a distance; everything else is a quality measure.
        assert!(FmetricsMetric::Butteraugli.prefers_lower_is_better());
        for metric in [
            FmetricsMetric::Ssimulacra2,
            FmetricsMetric::Cvvdp,
            FmetricsMetric::Iwssim,
            FmetricsMetric::Msssim,
        ] {
            assert!(
                !metric.prefers_lower_is_better(),
                "{metric:?} is higher-better"
            );
        }
    }

    #[test]
    fn only_cvvdp_is_temporal() {
        assert!(FmetricsMetric::Cvvdp.is_temporal());
        for metric in [
            FmetricsMetric::Ssimulacra2,
            FmetricsMetric::Butteraugli,
            FmetricsMetric::Iwssim,
            FmetricsMetric::Msssim,
        ] {
            assert!(!metric.is_temporal(), "{metric:?} must not be temporal");
        }
    }

    #[test]
    fn the_multi_scale_metrics_declare_a_minimum_size() {
        // They would otherwise fail inside the library with a code that does not
        // explain the cause.
        for metric in [FmetricsMetric::Iwssim, FmetricsMetric::Msssim] {
            let minimum = metric.minimum_dimension().expect("multi-scale needs a minimum");
            assert!(minimum >= 16, "{metric:?} minimum should be at least 16");
        }
        for metric in
            [FmetricsMetric::Ssimulacra2, FmetricsMetric::Butteraugli, FmetricsMetric::Cvvdp]
        {
            assert_eq!(
                metric.minimum_dimension(),
                None,
                "{metric:?} has no minimum"
            );
        }
    }

    #[test]
    fn a_frame_score_carries_each_metric_in_its_own_field() {
        for metric in [
            FmetricsMetric::Ssimulacra2,
            FmetricsMetric::Butteraugli,
            FmetricsMetric::Iwssim,
            FmetricsMetric::Msssim,
        ] {
            let score = FrameScore::with(metric, 1.25);
            assert_eq!(score.value(metric), Some(1.25), "{metric:?}");
            assert_eq!(FrameScore::empty().value(metric), None, "{metric:?}");
        }
    }

    #[test]
    fn cvvdp_reports_jod_as_the_headline() {
        // JOD is the headline measure, so it is what `value` returns.
        let score = FrameScore {
            ssimulacra2:   None,
            butteraugli:   None,
            cvvdp_jod:     Some(9.5),
            cvvdp_quality: Some(120.0),
            iwssim:        None,
            msssim:        None,
        };
        assert_eq!(score.value(FmetricsMetric::Cvvdp), Some(9.5));
        assert_eq!(score.cvvdp_quality, Some(120.0));
    }

    #[test]
    fn peak_sample_values_match_the_bit_depths() {
        assert_eq!(color(8).max_sample().unwrap(), 255);
        assert_eq!(color(10).max_sample().unwrap(), 1023);
        assert_eq!(color(12).max_sample().unwrap(), 4095);
    }

    #[test]
    fn a_frame_round_trips_through_conversion() {
        // Conversion is what every submission does first, so it must work on a
        // frame built exactly as the driver would supply one.
        let frames = frame(16, 16, 100);
        let info = color(8);
        // SAFETY: the planes are leaked buffers of the stated size.
        let image = unsafe { yuv_to_rgb(&frames, &info, 255).unwrap() };
        assert_eq!((image.width(), image.height()), (16, 16));
        assert_eq!(image.stride(), 48);
    }

    #[test]
    fn concurrent_scoring_matches_serial_scoring() {
        // fmetrics' scratch arena is per-workspace, so sharing one workspace
        // between threads corrupts it *silently*: no error is returned, the
        // scores are just wrong. A "next index" counter hands the same workspace
        // to two threads once they are far enough apart in the counter, which is
        // what made this test fail before the pool became a free list. Bit
        // equality is the only assertion strong enough to catch that.
        let Ok(scorer) = FmetricsScorer::new(
            FmetricsConfig::new(FmetricsMetric::Ssimulacra2).with_threads(4),
            color(8),
        ) else {
            // Without the library there is nothing to race.
            return;
        };

        let size = 64;
        let reference_bytes = vec![128u8; size * size * 3];
        let distorted_bytes = vec![100u8; size * size * 3];
        let reference = RgbView::new(&reference_bytes, size, size, false);
        let distorted = RgbView::new(&distorted_bytes, size, size, false);

        let expected = scorer.submit_rgb(&reference, &distorted).unwrap();

        let scorer = std::sync::Arc::new(scorer);
        // Four threads for four workspaces: oversubscribing would be reported
        // Four threads for four workspaces: oversubscribing would be reported
        // as exhaustion by design, which the pool test above covers.
        let results: Vec<FrameScore> = std::thread::scope(|scope| {
            (0..4)
                .map(|_| {
                    let scorer = std::sync::Arc::clone(&scorer);
                    scope.spawn(move || scorer.submit_rgb(&reference, &distorted).unwrap())
                })
                .map(|handle| handle.join().unwrap())
                .collect()
        });

        for score in &results {
            assert_eq!(
                score.ssimulacra2.unwrap().to_bits(),
                expected.ssimulacra2.unwrap().to_bits(),
                "a concurrent score differed from the serial one, so a workspace was shared"
            );
        }
    }

    #[test]
    fn a_pool_hands_each_workspace_out_exactly_once() {
        // The property the free list exists to provide. Claiming every workspace
        // must yield each handle once and then refuse, rather than wrapping round
        // to a workspace already in use -- a counter-based pool does exactly that,
        // and the resulting scores are wrong without any error to notice.
        let Ok(api) = FmetricsApi::load() else {
            return;
        };
        let mut workspaces = Vec::new();
        for _ in 0..3 {
            workspaces.push(Workspace::new(api).unwrap());
        }
        for (index, workspace) in workspaces.iter_mut().enumerate() {
            let successor = index + 1;
            workspace.next.store(
                if successor < 3 {
                    successor
                } else {
                    WorkspacePool::NONE
                },
                Ordering::Relaxed,
            );
        }
        let pool = WorkspacePool {
            workspaces,
            free: AtomicUsize::new(0),
        };

        let mut claimed = Vec::new();
        while let Some(handle) = pool.claim() {
            assert!(
                !claimed.contains(&handle),
                "the same workspace was handed out twice"
            );
            claimed.push(handle);
        }
        assert_eq!(claimed.len(), 3, "every workspace should be claimable once");

        // With everything in use the pool refuses rather than double-lending.
        assert!(pool.claim().is_none(), "an exhausted pool must refuse");

        // Releasing makes exactly one claimable again, and the freed one is next.
        let released = claimed[1];
        pool.release(released);
        assert_eq!(
            pool.claim(),
            Some(released),
            "the released workspace is next"
        );
        assert!(pool.claim().is_none(), "the rest are still held");
    }
}
