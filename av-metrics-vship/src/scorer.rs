//! Streaming per-pair vship scoring.
//!
//! libvship scores a reference/distorted pair through `Vship_ComputeHandler`
//! and writes the result into the struct it is handed. There is no sliding
//! window and no flush: the score for a pair is final the moment the call
//! returns. That is the central difference from the libvmaf binding, and it is
//! why no finish or batch step appears here.

use std::{
    ffi::{CString, c_char, c_int, c_void},
    marker::PhantomData,
    mem::size_of,
    ptr,
    sync::{Mutex, OnceLock},
};

use av_decoders::{Decoder, VideoDetails, v_frame::pixel::Pixel};
use v_frame::chroma::ChromaSubsampling;

use crate::{
    config::{
        ButteraugliParams,
        PoolMethod,
        VshipConfig,
        VshipMetric,
        colorspace_from_details,
        to_c_string,
    },
    error::VshipError,
    ffi::{
        self,
        VshipApi,
        VshipBackend,
        VshipColorspace,
        VshipDeviceInfo,
        VshipHandle,
        VshipInitButteraugli_1,
        VshipInitCvvdp_1,
        VshipInitSsimulacra2_1,
        VshipScoreButteraugli,
        VshipScoreCvvdp,
        VshipScoreSsimulacra2,
        VshipStructType,
    },
};

/// Sentinel for "no device was configured", so the default can be chosen rather
/// than assumed.
///
/// [`VshipConfig::gpu_id`] returns this until [`default_gpu_id`] has run, and
/// [`VshipConfig::gpu_id_or_default`] resolves it.
pub(crate) const GPU_UNSET: u32 = u32::MAX;

/// The device libvship would use when none is configured.
///
/// A machine with more than one GPU usually has a discrete part plus an
/// integrated one, and the discrete part is the one that can actually run these
/// metrics at a useful rate. libvship reports `integrated` for every backend,
/// so the highest-indexed non-integrated device is chosen, falling back to the
/// last device enumerated when every one reports integrated.
///
/// Returns `None` when libvship cannot be loaded or reports no devices at all,
/// rather than substituting an index. Every index this can return was read back
/// from `device_count`, so a zero here means device 0 does not exist, and
/// handing it to `gpu_full_check` or reporting it as the default would name a
/// device that is not there.
///
/// The result is cached in a `OnceLock`: the device configuration cannot change
/// while the process runs, so re-enumerating on each request would be waste.
#[inline]
#[must_use]
pub fn default_gpu_id() -> Option<u32> {
    static CACHE: OnceLock<Option<u32>> = OnceLock::new();
    *CACHE.get_or_init(choose_gpu_id)
}

/// Enumerate devices and pick a discrete one, or the last one available.
fn choose_gpu_id() -> Option<u32> {
    let api = VshipApi::load().ok()?;
    let count = api.device_count().ok()?;

    (0..count)
        .rev()
        .find(|&id| api.device_info(id).is_ok_and(|info| info.integrated == 0))
        .or_else(|| count.checked_sub(1))
}

/// Every compute device libvship can see, in libvship's own order.
///
/// The index of each device is the `gpu_id` that
/// [`VshipConfig::with_gpu_id`](crate::VshipConfig::with_gpu_id) takes, so this
/// is what a caller needs in order to select one by hand.
///
/// A device whose properties cannot be read is omitted rather than reported
/// with blanks. Each entry keeps the index it was enumerated at, so omitting
/// one never renumbers the others: a caller holding `gpu_id` 2 still finds
/// device 2 in this list.
///
/// # Errors
///
/// Returns [`VshipError::LibraryNotFound`] if libvship could not be loaded, and
/// [`VshipError::CallFailed`] if the device count could not be read.
#[inline]
pub fn devices() -> Result<Vec<VshipDevice>, VshipError> {
    let api = VshipApi::load()?;
    let count = api.device_count()?;

    Ok((0..count)
        .filter_map(|id| {
            api.device_info(id).ok().map(|info| VshipDevice {
                id,
                info,
            })
        })
        .collect())
}

/// One compute device libvship enumerated.
///
/// No `PartialEq`: [`VshipDeviceInfo`] is a raw `#[repr(C)]` mirror of a C
/// struct and does not derive it. Compare `id` and the fields read off `info`
/// instead.
#[derive(Debug, Clone, Copy)]
pub struct VshipDevice {
    /// The device's index, which is the `gpu_id` that selects it.
    pub id:   u32,
    /// What libvship reports about it.
    pub info: VshipDeviceInfo,
}

impl VshipDevice {
    /// Whether libvship classifies this device as integrated.
    #[inline]
    #[must_use]
    pub fn is_integrated(&self) -> bool {
        self.info.integrated != 0
    }
}

/// Geometry and format of one input.
///
/// The reference and the encode are described separately, because libvship
/// takes a colorspace per side and converts each independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFormat {
    /// Width in pixels.
    pub width:           u32,
    /// Height in pixels.
    pub height:          u32,
    /// Bits per channel: 8, 9, 10, 12, 14 or 16.
    pub bit_depth:       u32,
    /// Chroma subsampling.
    pub chroma_sampling: ChromaSubsampling,
}

impl VideoFormat {
    /// Build a format from a decoder's reported details.
    #[inline]
    #[must_use]
    pub fn from_details(details: &VideoDetails) -> Self {
        Self {
            width:           details.width as u32,
            height:          details.height as u32,
            bit_depth:       details.bit_depth as u32,
            chroma_sampling: details.chroma_sampling,
        }
    }

    /// The libvship colorspace for this format, optionally scaled to a target.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::UnsupportedFormat`] if the bit depth or chroma
    /// sampling has no libvship equivalent.
    #[inline]
    pub fn colorspace(
        &self,
        target_resolution: Option<(u32, u32)>,
    ) -> Result<VshipColorspace, VshipError> {
        colorspace_from_details(
            &VideoDetails {
                width:                         self.width as usize,
                height:                        self.height as usize,
                bit_depth:                     self.bit_depth as usize,
                chroma_sampling:               self.chroma_sampling,
                frame_rate:                    av_decoders::Rational32::new(0, 1),
                total_frames:                  None,
                color_range:                   None,
                color_primaries:               None,
                color_transfer_characteristic: None,
                color_space:                   None,
                chroma_location:               None,
                mastering_display:             None,
                content_light_level:           None,
            },
            target_resolution,
        )
    }
}

/// One plane of a frame: a strided block of samples with a visible origin.
///
/// This is the single description every supported frame source reduces to. A
/// `v_frame` `Plane` and a VapourSynth `Frame` plane are each just a pointer, a
/// stride and an origin, so one walk serves both and neither source needs its
/// own path through the scorer.
///
/// # Safety
///
/// `data` must be valid for reads covering every visible row, and must stay
/// valid until the value has been consumed. The type is `Copy`, so keep it
/// within a single expression rather than storing it.
#[derive(Debug, Clone, Copy)]
pub struct PlaneSource {
    /// Width in pixels, that is samples per row rather than bytes.
    width:            usize,
    /// Height in pixels.
    height:           usize,
    /// Bytes from one row to the next. May be negative for bottom-up images.
    stride:           isize,
    /// Offset of the first visible sample of row 0 from `data`.
    data_origin:      usize,
    /// Bytes per sample, as libvship sees them.
    bytes_per_sample: usize,
    /// Pointer to the start of the underlying allocation.
    data:             *const u8,
}

impl PlaneSource {
    /// Describe a plane from a raw pointer, stride and origin.
    ///
    /// # Safety
    ///
    /// `data` must remain readable for `height` rows at `stride` spacing, and
    /// `data_origin` must point at the first visible sample of row 0.
    #[inline]
    #[must_use]
    pub const unsafe fn new(
        width: usize,
        height: usize,
        stride: isize,
        data_origin: usize,
        bytes_per_sample: usize,
        data: *const u8,
    ) -> Self {
        Self {
            width,
            height,
            stride,
            data_origin,
            bytes_per_sample,
            data,
        }
    }

    /// Describe a plane that is contiguous, with no padding.
    ///
    /// # Safety
    ///
    /// `data` must be valid for reads of `height * width * bytes_per_sample`
    /// bytes.
    #[inline]
    #[must_use]
    pub const unsafe fn contiguous(
        width: usize,
        height: usize,
        bytes_per_sample: usize,
        data: *const u8,
    ) -> Self {
        // SAFETY: the caller's contract for this constructor is exactly the one
        // `new` requires.
        unsafe {
            Self::new(
                width,
                height,
                (width * bytes_per_sample) as isize,
                0,
                bytes_per_sample,
                data,
            )
        }
    }

    /// Width in pixels.
    #[inline]
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    #[inline]
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Bytes per sample.
    #[inline]
    #[must_use]
    pub const fn bytes_per_sample(&self) -> usize {
        self.bytes_per_sample
    }

    /// Pointer to the first visible byte of `row`, or `None` when out of range.
    #[inline]
    fn row(&self, row: usize) -> Option<*const u8> {
        if row >= self.height {
            return None;
        }
        // SAFETY: the constructor's contract guarantees the allocation spans
        // `height` rows at `stride` spacing, and `row` is bounds-checked above.
        Some(unsafe { self.data.add(self.data_origin + (row as isize * self.stride) as usize) })
    }
}

/// A frame as the three planes libvship scores.
///
/// Every representation a caller might have reduces to this, which is why the
/// scorer needs no per-source special cases.
pub type PlaneSet = [PlaneSource; 3];

/// Describe a `v_frame` plane as a [`PlaneSource`].
fn plane_source<T: Pixel>(
    plane: &v_frame::plane::Plane<T>,
    bytes_per_sample: usize,
) -> PlaneSource {
    let geometry = plane.geometry();

    // SAFETY: `Plane::data` is the plane's own allocation, and the geometry's
    // stride and origin describe exactly that allocation. The `PlaneSource` is
    // built and consumed inside the same submit call, so the borrow outlives it.
    //
    // `v_frame` reports stride and origin in samples, so both are scaled to bytes
    // here. The stride is widened rather than cast, because the field is signed
    // to allow bottom-up planes from other frame sources.
    unsafe {
        PlaneSource::new(
            geometry.width(),
            geometry.height(),
            geometry.stride() as isize * bytes_per_sample as isize,
            geometry.data_origin() * bytes_per_sample,
            bytes_per_sample,
            plane.data().as_ptr().cast::<u8>(),
        )
    }
}

/// A single pair's score.
///
/// The fields present depend on the metric; the unused ones are `None` rather
/// than a zero, since a zero would be indistinguishable from a real score.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FrameScore {
    /// SSIMULACRA2's value, or CVVDP's running mean. `None` for Butteraugli.
    pub score:    Option<f64>,
    /// Butteraugli's configured-norm value.
    pub norm_q:   Option<f64>,
    /// Butteraugli's L3 norm.
    pub norm_3:   Option<f64>,
    /// Butteraugli's Linfinity norm.
    pub norm_inf: Option<f64>,
}

impl FrameScore {
    /// The value to pool or report for this pair.
    ///
    /// For Butteraugli this is [`Self::butteraugli_value`] rather than a fixed
    /// field, because which norm is the headline depends on the configuration
    /// and not on the metric: `q_norm` sets the exponent libvship
    /// minimises, while the VapourSynth plugin reports `norm_inf` unless a
    /// norm was requested explicitly.
    #[inline]
    #[must_use]
    pub const fn value(&self, metric: VshipMetric) -> Option<f64> {
        match metric {
            VshipMetric::Butteraugli => self.butteraugli_value(true),
            VshipMetric::Ssimulacra2 | VshipMetric::Cvvdp => self.score,
        }
    }

    /// The value to pool or report for Butteraugli.
    ///
    /// With `q_norm` set, libvship's Q-norm is the quantity the metric
    /// minimises and is what should be pooled. Without it, the reported
    /// value is the infinity norm: that is what the VapourSynth plugin
    /// exposes as `BUTTERAUGLI_INFNorm` when no norm is requested, and it
    /// is what `av-metrics-vship`'s caller is expected to pool, so that
    /// both paths agree.
    #[inline]
    #[must_use]
    pub const fn butteraugli_value(&self, use_q_norm: bool) -> Option<f64> {
        if use_q_norm {
            self.norm_q
        } else {
            self.norm_inf
        }
    }

    /// The headline value, or `None` when the metric produced none.
    #[inline]
    #[must_use]
    pub fn primary(&self) -> Option<f64> {
        // `Option::or` is not const, so the first present value is found by hand.
        if self.score.is_some() {
            self.score
        } else {
            self.norm_q
        }
    }
}

/// The NUL-terminated strings CVVDP's init struct points at.
///
/// Held beside the pool so the pointers libvship is given outlive the calls
/// that use them.
struct CvvdpStrings {
    /// Display model key, always present.
    model_key:                  CString,
    /// Display model JSON path, or null when none is configured.
    model_config_json:          *const c_char,
    /// Backing storage for `model_config_json`, kept alive beside the pointer.
    _model_config_json_storage: Option<CString>,
}

impl CvvdpStrings {
    /// Build the strings from a configuration.
    fn new(config: &VshipConfig) -> Result<Self, VshipError> {
        let model_key = to_c_string(&config.cvvdp().display_model, "display model")?;
        let json = config.cvvdp().display_model_json.as_str();

        let (model_config_json, storage) = if json.is_empty() {
            (ptr::null(), None)
        } else {
            let storage = to_c_string(json, "display model JSON path")?;
            (storage.as_ptr(), Some(storage))
        };

        Ok(Self {
            model_key,
            model_config_json,
            _model_config_json_storage: storage,
        })
    }
}

/// One libvship handler plus the colorspaces it was built for.
///
/// The handler pointer is owned here and released exactly once by [`Drop`]. It
/// is not `Send` on its own, because libvship documents a handler as belonging
/// to one thread; the pool below instead gives each worker its own handler.
struct MetricHandler {
    handle:         VshipHandle,
    src_colorspace: VshipColorspace,
    dis_colorspace: VshipColorspace,
}

impl MetricHandler {
    /// Create a handler for `metric` and the given colorspaces.
    fn new(
        api: &'static VshipApi,
        metric: VshipMetric,
        config: &VshipConfig,
        src_colorspace: VshipColorspace,
        dis_colorspace: VshipColorspace,
        cvvdp_strings: &CvvdpStrings,
    ) -> Result<Self, VshipError> {
        let gpu_id = config.gpu_id_or_default() as c_int;
        let mut handle: VshipHandle = ptr::null_mut();

        // The init struct is metric-agnostic on the C side: `Vship_InitHandler`
        // reads its `struct_type` discriminator to learn which layout it was
        // handed, so all three variants are built here and one is selected.
        let status = match metric {
            VshipMetric::Ssimulacra2 => {
                let init = VshipInitSsimulacra2_1 {
                    struct_type: VshipStructType::InitSsimulacra2,
                    src_colorspace,
                    dis_colorspace,
                    gpu_id,
                };
                // SAFETY: `init` is a live `VshipInitSsimulacra2_1` whose
                // discriminator matches the metric, and `handle` is a writable
                // out-pointer for one `Vship_Handle`.
                unsafe {
                    (api.init_handler)(
                        &raw mut handle,
                        std::ptr::from_ref(&init).cast_mut().cast::<c_void>(),
                    )
                }
            },
            VshipMetric::Butteraugli => {
                let ButteraugliParams {
                    q_norm,
                    intensity_multiplier,
                } = config.butteraugli();

                let init = VshipInitButteraugli_1 {
                    struct_type: VshipStructType::InitButteraugli,
                    src_colorspace,
                    dis_colorspace,
                    q_norm: *q_norm,
                    intensity_multiplier: *intensity_multiplier,
                    gpu_id,
                };
                // SAFETY: as above, for the Butteraugli layout.
                unsafe {
                    (api.init_handler)(
                        &raw mut handle,
                        std::ptr::from_ref(&init).cast_mut().cast::<c_void>(),
                    )
                }
            },
            VshipMetric::Cvvdp => {
                let init = VshipInitCvvdp_1 {
                    struct_type: VshipStructType::InitCvvdp,
                    src_colorspace,
                    dis_colorspace,
                    fps: config.cvvdp().fps,
                    resize_to_display: config.cvvdp().resize_to_display,
                    model_key_cstr: cvvdp_strings.model_key.as_ptr(),
                    model_config_json_cstr: cvvdp_strings.model_config_json,
                    gpu_id,
                };
                // SAFETY: as above, for the CVVDP layout. Both string pointers
                // are NUL-terminated and outlive the call through `CvvdpStrings`.
                unsafe {
                    (api.init_handler)(
                        &raw mut handle,
                        std::ptr::from_ref(&init).cast_mut().cast::<c_void>(),
                    )
                }
            },
        };

        if let Err(error) = ffi::check(status, "Vship_InitHandler") {
            return Err(VshipError::MetricUnavailable {
                metric: metric.as_str().to_owned(),
                reason: error.to_string(),
            });
        }

        Ok(Self {
            handle,
            src_colorspace,
            dis_colorspace,
        })
    }

    /// Score one pair and return the value libvship wrote.
    fn compute(
        &mut self,
        api: &'static VshipApi,
        metric: VshipMetric,
        reference: PlaneSet,
        distorted: PlaneSet,
    ) -> Result<FrameScore, VshipError> {
        validate_planes(&reference, &self.src_colorspace, "reference")?;
        validate_planes(&distorted, &self.dis_colorspace, "distorted")?;

        // libvship reads three plane pointers and two line-size arrays per side,
        // all of which must stay live for the call.
        let (srcp1, line_size) = plane_arguments(&reference);
        let (srcp2, line_size2) = plane_arguments(&distorted);

        let mut score = FrameScore::default();

        match metric {
            VshipMetric::Ssimulacra2 => {
                let mut out = VshipScoreSsimulacra2::default();
                // SAFETY: the argument arrays are live locals of exactly the
                // pointer and `i64` types the signature names, `out` is a writable
                // score struct whose discriminator matches the metric, and the
                // handler is owned by `self`.
                let status = unsafe {
                    (api.compute_handler)(
                        self.handle,
                        std::ptr::from_mut(&mut out).cast::<c_void>(),
                        srcp1.as_ptr(),
                        srcp2.as_ptr(),
                        line_size.as_ptr(),
                        line_size2.as_ptr(),
                    )
                };
                ffi::check(status, "Vship_ComputeHandler")?;
                score.score = Some(out.score);
            },
            VshipMetric::Butteraugli => {
                // `dstp` stays null, which the C header defines as never copying
                // the distortion map back from the device.
                let mut out = VshipScoreButteraugli::default();
                // SAFETY: as above, for the Butteraugli score layout.
                let status = unsafe {
                    (api.compute_handler)(
                        self.handle,
                        std::ptr::from_mut(&mut out).cast::<c_void>(),
                        srcp1.as_ptr(),
                        srcp2.as_ptr(),
                        line_size.as_ptr(),
                        line_size2.as_ptr(),
                    )
                };
                ffi::check(status, "Vship_ComputeHandler")?;
                score.norm_q = Some(out.norm_q);
                score.norm_3 = Some(out.norm_3);
                score.norm_inf = Some(out.norm_inf);
            },
            VshipMetric::Cvvdp => {
                let mut out = VshipScoreCvvdp::default();
                // SAFETY: as above, for the CVVDP score layout.
                let status = unsafe {
                    (api.compute_handler)(
                        self.handle,
                        std::ptr::from_mut(&mut out).cast::<c_void>(),
                        srcp1.as_ptr(),
                        srcp2.as_ptr(),
                        line_size.as_ptr(),
                        line_size2.as_ptr(),
                    )
                };
                ffi::check(status, "Vship_ComputeHandler")?;
                score.score = Some(out.score);
            },
        }

        Ok(score)
    }

    /// Clear this handler's temporal frame history.
    fn reset(&self, api: &'static VshipApi) -> Result<(), VshipError> {
        // SAFETY: `handle` is owned by `self` and `Vship_Reset` takes no other
        // arguments.
        let status = unsafe { (api.reset)(self.handle) };
        ffi::check(status, "Vship_Reset")
    }

    /// Clear this handler's accumulated score, leaving its frame history.
    fn reset_score(&self, api: &'static VshipApi) -> Result<(), VshipError> {
        // SAFETY: as `reset`.
        let status = unsafe { (api.reset_score)(self.handle) };
        ffi::check(status, "Vship_ResetScore")
    }
}

impl Drop for MetricHandler {
    #[inline]
    fn drop(&mut self) {
        // libvship rejects freeing a null handle, so one that never initialised
        // is skipped rather than passed on.
        if self.handle.is_null() {
            return;
        }

        if let Ok(api) = VshipApi::load() {
            // SAFETY: `handle` came from `Vship_InitHandler` and is released
            // exactly once, here.
            unsafe { (api.free_handler)(self.handle) };
        }
    }
}

/// Turn a plane set into the pointer and line-size arrays libvship expects.
///
/// libvship walks the planes itself, so only the base pointer and the row
/// stride are passed; the data origin is folded into the base pointer.
#[inline]
fn plane_arguments(planes: &PlaneSet) -> ([*const u8; 3], [i64; 3]) {
    let mut pointers = [ptr::null(); 3];
    let mut line_sizes = [0i64; 3];

    for (index, plane) in planes.iter().enumerate() {
        // Row 0 is the first row libvship reads, and its offset from the base
        // pointer is exactly the visible origin. A zero-height plane has no row 0,
        // so it contributes a null pointer, which libvship treats as absent.
        // SAFETY: `row` is bounds-checked inside, and the constructor's contract
        // guarantees the returned pointer addresses a valid visible row.
        pointers[index] = plane.row(0).unwrap_or(ptr::null());
        line_sizes[index] = plane.stride as i64;
    }

    (pointers, line_sizes)
}

/// Check that a plane set matches the geometry its colorspace declared.
fn validate_planes(
    planes: &PlaneSet,
    colorspace: &VshipColorspace,
    side: &str,
) -> Result<(), VshipError> {
    let (chroma_width, chroma_height) = chroma_dimensions(colorspace)?;

    for (index, plane) in planes.iter().enumerate() {
        let (expected_width, expected_height) = if index == 0 {
            (colorspace.width, colorspace.height)
        } else {
            (chroma_width, chroma_height)
        };

        if i64::try_from(plane.width).unwrap_or(-1) != expected_width
            || i64::try_from(plane.height).unwrap_or(-1) != expected_height
        {
            return Err(VshipError::InputMismatch {
                reason: format!(
                    "{side} plane {index} is {}x{} but its colorspace declares \
                     {expected_width}x{expected_height}",
                    plane.width, plane.height
                ),
            });
        }
    }

    Ok(())
}

/// The chroma plane dimensions implied by a colorspace.
#[inline]
fn chroma_dimensions(colorspace: &VshipColorspace) -> Result<(i64, i64), VshipError> {
    let shift = |value: c_int| {
        u32::try_from(value).map_err(|_| VshipError::MalformedFrame {
            reason: format!("negative chroma subsampling exponent {value}"),
        })
    };

    let subw = shift(colorspace.subsampling.subw)?;
    let subh = shift(colorspace.subsampling.subh)?;

    // An exponent wider than the dimension itself is a corrupted colorspace
    // rather than a real subsampling.
    if subw > 8 || subh > 8 {
        return Err(VshipError::MalformedFrame {
            reason: format!("implausible chroma subsampling exponent {subw}x{subh}"),
        });
    }

    Ok((colorspace.width >> subw, colorspace.height >> subh))
}

/// The handler pool.
struct Pool {
    /// The handlers, indexed by worker.
    handlers:   Vec<MetricHandler>,
    /// The next handler to use, for a non-temporal metric.
    next:       usize,
    /// The index the pool last saw, or `None` before the first pair.
    last_index: Option<u32>,
    /// How many pairs have been submitted, and so the next sequential index.
    submitted:  u32,
}

/// A vship scoring session.
///
/// Scores are produced by streaming matching frame pairs from a reference and a
/// distorted [`Decoder`]. libvship scores a pair as it arrives, so the value
/// returned by [`Self::submit_pair`] is final immediately: there is no flush
/// and no sliding window.
///
/// # Threading
///
/// A non-temporal metric builds [`VshipConfig::handler_threads`] handlers, one
/// per worker, and distributes pairs across them round-robin. CVVDP is
/// temporal: it builds exactly one handler, sees frames in submission order,
/// and resets its history at an index discontinuity so a scene break does not
/// corrupt the score.
///
/// A `VshipScorer` is [`Send`] so it can be moved into a worker, but it is not
/// [`Sync`]: one scorer owns its whole pool and drives it serially.
pub struct VshipScorer {
    api:       &'static VshipApi,
    config:    VshipConfig,
    metric:    VshipMetric,
    /// The handlers, behind a mutex so the pool can be moved between threads
    /// while each handler stays owned by one of them.
    handlers:  Mutex<Pool>,
    /// Ensures `!Sync`, because a scorer drives its pool serially.
    _not_sync: PhantomData<*mut ()>,
}

impl VshipScorer {
    /// Create a scorer for the given reference and encode formats.
    ///
    /// `target_resolution` is Av1an's encode resolution, which becomes
    /// libvship's `target_width` and `target_height`.
    ///
    /// The two sides need not share a format, but they must end up the same
    /// size: libvship does not resize implicitly and fails
    /// `Vship_InitHandler` with `DifferingInputType` when the two
    /// colorspaces resolve to different dimensions. When
    /// `target_resolution` is `None` and the formats differ in
    /// size, the reference size is used as the target, so a 1080p reference can
    /// be scored against a 720p encode without the caller having to say so.
    /// Pass an explicit target to scale to some third size instead.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::LibraryNotFound`] if libvship could not be loaded,
    /// [`VshipError::InvalidConfiguration`] if the configuration is unusable,
    /// [`VshipError::UnsupportedFormat`] if either format is outside libvship's
    /// supported set, and [`VshipError::MetricUnavailable`] if a handler cannot
    /// be created.
    #[expect(
        clippy::missing_inline_in_public_items,
        reason = "inlining the scorer would duplicate the whole body at every call site"
    )]
    pub fn new(
        config: VshipConfig,
        reference: VideoFormat,
        encode: VideoFormat,
        target_resolution: Option<(u32, u32)>,
    ) -> Result<Self, VshipError> {
        config.validate()?;
        let api = VshipApi::load()?;

        api.gpu_full_check(config.gpu_id_or_default())?;

        // libvship performs no geometry-equality check between the two sides: it
        // converts each per its own colorspace and then rejects the pair with
        // `DifferingInputType` if the results differ in size. Scaling is therefore
        // explicit, and an explicit target scales both sides to that size. With no
        // target, a mismatched pair would fail, so the reference size is adopted --
        // the same choice FFVship makes, which always tells libvship to scale the
        // encode to the source. Pairs that already match are left untouched.
        let target_resolution = target_resolution.or_else(|| {
            (reference.width != encode.width || reference.height != encode.height)
                .then_some((reference.width, reference.height))
        });

        let src_colorspace = reference.colorspace(target_resolution)?;
        let dis_colorspace = encode.colorspace(target_resolution)?;
        let cvvdp_strings = CvvdpStrings::new(&config)?;

        let count = config.handler_count();
        let mut handlers = Vec::with_capacity(count);
        for _ in 0..count {
            handlers.push(MetricHandler::new(
                api,
                config.metric(),
                &config,
                src_colorspace,
                dis_colorspace,
                &cvvdp_strings,
            )?);
        }

        let metric = config.metric();

        Ok(Self {
            api,
            config,
            metric,
            handlers: Mutex::new(Pool {
                handlers,
                next: 0,
                last_index: None,
                submitted: 0,
            }),
            _not_sync: PhantomData,
        })
    }

    /// The backend this libvship build targets.
    #[inline]
    #[must_use]
    pub fn backend(&self) -> VshipBackend {
        self.api.backend()
    }

    /// The device this scorer was built for.
    #[inline]
    #[must_use]
    pub fn gpu_id(&self) -> u32 {
        self.config.gpu_id_or_default()
    }

    /// The configuration in use.
    #[inline]
    #[must_use]
    pub const fn config(&self) -> &VshipConfig {
        &self.config
    }

    /// The metric being computed.
    #[inline]
    #[must_use]
    pub const fn metric(&self) -> VshipMetric {
        self.metric
    }

    /// How many handlers this scorer owns.
    ///
    /// Zero only if the pool's lock was poisoned by a panic elsewhere.
    #[inline]
    #[must_use]
    pub fn handler_count(&self) -> usize {
        // A poisoned lock is reported by [`Self::submitted`] rather than here,
        // so this is a diagnostic count and must not itself panic.
        self.handlers.lock().map_or(0, |pool| pool.handlers.len())
    }

    /// Whether libvship is available on this system.
    ///
    /// This needs more than a successful `dlopen`: `Vship_GetVersion` must
    /// answer and `Vship_GPUFullCheck` must pass on a device. Any of those
    /// failing reports unavailability rather than panicking.
    ///
    /// Cheap to call: the library is resolved once and cached.
    #[inline]
    #[must_use]
    pub fn is_available() -> bool {
        default_gpu_id().is_some_and(|gpu_id| availability(gpu_id).is_ok())
    }

    /// The runtime libvship version string, if available.
    #[inline]
    #[must_use]
    pub fn libvship_version() -> Option<&'static str> {
        default_gpu_id().and_then(|gpu_id| availability(gpu_id).ok())
    }

    /// The name of the device libvship would use, if it is available.
    #[inline]
    #[must_use]
    pub fn device_name() -> Option<String> {
        let api = VshipApi::load().ok()?;
        api.device_info(default_gpu_id()?).ok().map(|info| info.name())
    }

    /// What libvship reports about the device it would use.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::LibraryNotFound`] if libvship could not be loaded,
    /// and [`VshipError::CallFailed`] if no device is available or the device
    /// cannot be queried.
    #[inline]
    pub fn device_info() -> Result<VshipDeviceInfo, VshipError> {
        let api = VshipApi::load()?;
        let gpu_id = default_gpu_id().ok_or_else(|| VshipError::CallFailed {
            function: "Vship_GetDeviceCount",
            status:   0,
            message:  "libvship reported no compute devices".to_owned(),
        })?;

        api.device_info(gpu_id)
    }

    /// Score every matching frame pair from the two decoders.
    ///
    /// Returns one [`FrameScore`] per pair, in submission order. Each is
    /// complete the moment `Vship_ComputeHandler` returns, so the callback
    /// fires per pair and the final score needs no flush.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::FrameCountMismatch`] if the clips have different
    /// lengths, and [`VshipError::Decode`] or [`VshipError::CallFailed`] on
    /// downstream failures.
    #[expect(
        clippy::missing_inline_in_public_items,
        reason = "inlining the scoring loop would duplicate the whole body"
    )]
    pub fn score_decoders<T: Pixel>(
        &mut self,
        reference: &mut Decoder,
        distorted: &mut Decoder,
        mut on_score: impl FnMut(usize, &FrameScore),
    ) -> Result<Vec<FrameScore>, VshipError> {
        let mut scores = Vec::new();

        loop {
            let reference_frame = reference.read_video_frame::<T>();
            let distorted_frame = distorted.read_video_frame::<T>();

            let (reference_frame, distorted_frame) = match (reference_frame, distorted_frame) {
                (Ok(reference_frame), Ok(distorted_frame)) => (reference_frame, distorted_frame),
                (reference_result, distorted_result) => {
                    // `av-decoders` signals end of stream through
                    // `DecoderError`, not an `Option`, so a short read is only
                    // distinguishable via its error.
                    let reference_done =
                        matches!(reference_result, Err(av_decoders::DecoderError::EndOfFile));
                    let distorted_done =
                        matches!(distorted_result, Err(av_decoders::DecoderError::EndOfFile));

                    if reference_done && distorted_done {
                        break;
                    }

                    if reference_done || distorted_done {
                        let scored = scores.len();
                        return Err(VshipError::FrameCountMismatch {
                            reference: scored + usize::from(!reference_done),
                            distorted: scored + usize::from(!distorted_done),
                        });
                    }

                    // A genuine decode failure on one or both sides; surface the
                    // reference's, falling back to the distorted's.
                    let error = match (reference_result, distorted_result) {
                        (Err(error), _) => error,
                        (_, Err(error)) => error,
                        // Unreachable: both arms above require an `Err`, and the
                        // Ok/Ok case was matched earlier.
                        (Ok(_), Ok(_)) => av_decoders::DecoderError::EndOfFile,
                    };
                    return Err(error.into());
                },
            };

            let frame_score = self.submit_pair(&reference_frame, &distorted_frame)?;
            on_score(scores.len(), &frame_score);
            scores.push(frame_score);
        }

        Ok(scores)
    }

    /// Pool a scorer's per-frame scores into a single value.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::InvalidConfiguration`] if `scores` is empty.
    #[inline]
    pub fn pool(
        scores: &[FrameScore],
        metric: VshipMetric,
        method: PoolMethod,
    ) -> Result<f64, VshipError> {
        let values: Vec<f64> = scores.iter().filter_map(|score| score.value(metric)).collect();

        method.apply(&values)
    }

    /// Pool Butteraugli using an explicitly chosen norm.
    ///
    /// [`Self::pool`] always takes the Q-norm for Butteraugli, which is right
    /// when `q_norm` is configured. A caller that leaves the norm unset
    /// needs [`FrameScore::norm_inf`] instead, to match what the
    /// VapourSynth plugin reports, and this is the entry point that selects
    /// it.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::InvalidConfiguration`] if no score produced a
    /// value for `method`.
    #[inline]
    pub fn pool_butteraugli(
        scores: &[FrameScore],
        use_q_norm: bool,
        method: PoolMethod,
    ) -> Result<f64, VshipError> {
        let values: Vec<f64> =
            scores.iter().filter_map(|score| score.butteraugli_value(use_q_norm)).collect();

        method.apply(&values)
    }

    /// Submit one frame pair and return its score.
    ///
    /// This is the low-level primitive behind [`Self::score_decoders`]. Prefer
    /// that unless the caller needs to interleave decoding with submission,
    /// which is what makes memory use independent of clip length: only the
    /// pool and the one pair in flight are live.
    ///
    /// Pairs must be submitted in index order starting at zero. For a temporal
    /// metric an index discontinuity resets the handler's history, since the
    /// skipped frames are ones it never saw.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::InputMismatch`] if a plane's geometry disagrees
    /// with its colorspace, and [`VshipError::CallFailed`] if libvship
    /// rejects the pair.
    #[expect(
        clippy::missing_inline_in_public_items,
        reason = "the pool dispatch and temporal bookkeeping belong in one body"
    )]
    pub fn submit_pair<T: Pixel>(
        &mut self,
        reference: &v_frame::frame::Frame<T>,
        distorted: &v_frame::frame::Frame<T>,
    ) -> Result<FrameScore, VshipError> {
        let bytes_per_sample = size_of::<T>();

        // `Frame` exposes chroma planes as `Option`, since monochrome frames have
        // none. libvship requires chroma, so this is a validation failure.
        let (Some(u_plane), Some(v_plane)) = (&reference.u_plane, &reference.v_plane) else {
            return Err(VshipError::UnsupportedFormat {
                reason: "reference frame has no chroma planes; libvship requires 4:2:0, 4:2:2 or \
                         4:4:4 content"
                    .to_owned(),
            });
        };
        let (Some(d_u), Some(d_v)) = (&distorted.u_plane, &distorted.v_plane) else {
            return Err(VshipError::UnsupportedFormat {
                reason: "distorted frame has no chroma planes; libvship requires 4:2:0, 4:2:2 or \
                         4:4:4 content"
                    .to_owned(),
            });
        };

        let reference_planes = [
            plane_source(&reference.y_plane, bytes_per_sample),
            plane_source(u_plane, bytes_per_sample),
            plane_source(v_plane, bytes_per_sample),
        ];
        let distorted_planes = [
            plane_source(&distorted.y_plane, bytes_per_sample),
            plane_source(d_u, bytes_per_sample),
            plane_source(d_v, bytes_per_sample),
        ];

        self.submit_planes(reference_planes, distorted_planes)
    }

    /// Submit one frame pair from raw plane sources.
    ///
    /// The entry point for callers whose frames are not `v_frame::Frame`s. A
    /// VapourSynth `Frame` is the case that matters: its planes are already
    /// strided buffers, so pointing at them directly avoids the per-plane copy
    /// that `av_decoders` performs to produce a `v_frame::Frame`.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::InputMismatch`] if a plane's geometry disagrees
    /// with its colorspace, and [`VshipError::CallFailed`] if libvship
    /// rejects the pair.
    #[inline]
    pub fn submit_pair_raw(
        &mut self,
        reference: PlaneSet,
        distorted: PlaneSet,
    ) -> Result<FrameScore, VshipError> {
        self.submit_planes(reference, distorted)
    }

    /// How many pairs have been submitted so far.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::InvalidConfiguration`] if the pool's lock was
    /// poisoned by a panic elsewhere.
    #[inline]
    pub fn submitted(&self) -> Result<u32, VshipError> {
        self.handlers.lock().map(|pool| pool.submitted).map_err(|_| poisoned())
    }

    /// Clear the temporal handler's frame history, as a scene break would
    /// require.
    ///
    /// The accumulated score survives: the C header defines `Vship_Reset` as
    /// emptying "temporal filter history of temporal metrics (but not score
    /// accumulation)", so the running mean continues across the break. Use
    /// [`Self::reset_score`] as well when a scene's score must cover only
    /// itself.
    ///
    /// Meaningful only for a temporal metric; for the others libvship holds no
    /// frame state to clear, so this is a no-op rather than an error.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::CallFailed`] if libvship rejects the reset.
    #[inline]
    pub fn reset_temporal(&self) -> Result<(), VshipError> {
        let Ok(mut pool) = self.handlers.lock() else {
            return Err(poisoned());
        };

        for handler in &pool.handlers {
            handler.reset(self.api)?;
        }
        // Clearing the recorded index means the next submission starts a fresh
        // sequence rather than being read as a jump.
        pool.last_index = None;
        Ok(())
    }

    /// Clear the temporal handler's accumulated score, leaving its frame
    /// history.
    ///
    /// # Errors
    ///
    /// Returns [`VshipError::CallFailed`] if libvship rejects the reset.
    #[inline]
    pub fn reset_score(&self) -> Result<(), VshipError> {
        let Ok(pool) = self.handlers.lock() else {
            return Err(poisoned());
        };

        for handler in &pool.handlers {
            handler.reset_score(self.api)?;
        }
        Ok(())
    }

    /// The single path every submitter funnels through.
    fn submit_planes(
        &mut self,
        reference: PlaneSet,
        distorted: PlaneSet,
    ) -> Result<FrameScore, VshipError> {
        let Ok(mut pool) = self.handlers.lock() else {
            return Err(poisoned());
        };

        let index = pool.submitted;

        // A temporal handler must see every frame in order, so a jump in the index
        // means frames it never saw would otherwise follow frames it has. That is
        // exactly what a scene break looks like from its point of view, and
        // clearing the history is far cheaper than recreating the handler.
        let discontinuous = pool.last_index.is_some_and(|last| last != index.wrapping_sub(1));
        if discontinuous && self.metric.is_temporal() && self.config.cvvdp().reset_on_discontinuity
        {
            for handler in &pool.handlers {
                handler.reset(self.api)?;
            }
        }

        // A non-temporal metric alternates handlers so the device sees parallel
        // work; a temporal one owns a single handler that always takes the pair.
        let slot = if self.metric.is_temporal() {
            0
        } else {
            let slot = pool.next % pool.handlers.len();
            pool.next = pool.next.wrapping_add(1);
            slot
        };

        pool.last_index = Some(index);
        pool.submitted = index.wrapping_add(1);

        pool.handlers[slot].compute(self.api, self.metric, reference, distorted)
    }
}

/// The error reported when the pool's lock was poisoned by a panic.
#[inline]
fn poisoned() -> VshipError {
    VshipError::InvalidConfiguration {
        reason: "the vship handler pool was left poisoned by a panic".to_owned(),
    }
}

/// Verify that libvship is loadable and that a device can actually run it.
///
/// # Errors
///
/// Returns [`VshipError::LibraryNotFound`] when the library cannot be loaded,
/// and [`VshipError::CallFailed`] when `Vship_GPUFullCheck` fails.
#[inline]
fn availability(gpu_id: u32) -> Result<&'static str, VshipError> {
    static CACHED_VERSION: OnceLock<String> = OnceLock::new();

    let api = VshipApi::load()?;
    // The version is queried before the device check, so a library that loaded
    // but cannot run reports as unavailable rather than as a working scorer.
    let version = CACHED_VERSION.get_or_init(|| api.version_string()).clone();
    api.gpu_full_check(gpu_id)?;

    // The string is leaked once so callers can hold it as `&'static str`, which
    // matches how the version is cached in the libvmaf binding too.
    Ok(Box::leak(version.into_boxed_str()))
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "a failing assertion should panic loudly, which is what a unit test wants"
)]
mod tests {
    use super::*;

    /// Video details in the shape `av-decoders` reports them.
    fn details(
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
            frame_rate: av_decoders::Rational32::new(24, 1),
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

    /// A discrete device must be preferred over an integrated one.
    ///
    /// libvship exposes `integrated` on every backend, and most multi-GPU
    /// systems pair a discrete part with an integrated one, so the
    /// highest-indexed non-integrated device is what the default has to
    /// resolve to.
    #[test]
    fn the_default_device_prefers_a_discrete_gpu() {
        if VshipApi::load().is_err() {
            return;
        }

        let Ok(api) = VshipApi::load() else { return };
        let Ok(count) = api.device_count() else {
            return;
        };
        // No devices means no index to choose, which is reported rather than
        // silently resolved to a device that does not exist.
        let Some(chosen) = default_gpu_id() else {
            assert_eq!(count, 0, "a device exists, so one must have been chosen");
            return;
        };

        assert!(
            chosen < count,
            "the chosen device {chosen} must exist among {count}"
        );
        if let Ok(info) = api.device_info(chosen) {
            // Only asserted when a discrete part is actually present, since a
            // single-GPU or all-integrated machine must still resolve.
            let any_discrete =
                (0..count).any(|id| api.device_info(id).is_ok_and(|info| info.integrated == 0));
            if any_discrete {
                assert_eq!(
                    info.integrated, 0,
                    "a discrete device is available, so the default must not be integrated"
                );
            }
        }
    }

    /// The resolved device must be stable, since it is cached.
    #[test]
    fn the_default_device_is_cached() {
        assert_eq!(default_gpu_id(), default_gpu_id());
    }

    #[test]
    fn video_format_reads_decoder_details() {
        let format = VideoFormat::from_details(&details(1920, 1080, 10, ChromaSubsampling::Yuv420));

        assert_eq!(format.width, 1920);
        assert_eq!(format.height, 1080);
        assert_eq!(format.bit_depth, 10);
        assert_eq!(format.chroma_sampling, ChromaSubsampling::Yuv420);
    }

    #[test]
    fn video_format_maps_to_a_colorspace() {
        let format = VideoFormat::from_details(&details(64, 48, 8, ChromaSubsampling::Yuv420));
        let colorspace = format.colorspace(Some((32, 24))).expect("8-bit 4:2:0 is supported");

        assert_eq!(colorspace.width, 64);
        assert_eq!(colorspace.target_width, 32);
        assert_eq!(colorspace.target_height, 24);
    }

    #[test]
    fn plane_arguments_report_the_origin_and_stride() {
        let luma = [0u8; 16];
        let chroma = [0u8; 4];

        // SAFETY: each buffer is larger than the plane it describes, and the
        // `PlaneSet` is consumed within this expression.
        let planes: PlaneSet = unsafe {
            [
                PlaneSource::new(4, 4, 4, 0, 1, luma.as_ptr()),
                PlaneSource::new(2, 2, 2, 0, 1, chroma.as_ptr()),
                PlaneSource::contiguous(2, 2, 1, chroma.as_ptr()),
            ]
        };

        let (pointers, strides) = plane_arguments(&planes);
        assert_eq!(pointers[0], luma.as_ptr());
        assert_eq!(pointers[1], chroma.as_ptr());
        assert_eq!(strides, [4, 2, 2]);
    }

    /// A plane whose data starts partway into its allocation must have that
    /// origin folded into the pointer libvship receives, since libvship walks
    /// the rows itself from a base address.
    #[test]
    fn plane_arguments_fold_in_the_data_origin() {
        let luma = [0u8; 16];
        let chroma = [0u8; 4];

        // SAFETY: the origin offset plus the described geometry stays inside the
        // buffer, and the `PlaneSet` is consumed within this expression.
        let planes: PlaneSet = unsafe {
            [
                PlaneSource::new(2, 2, 2, 1, 1, luma.as_ptr()),
                PlaneSource::contiguous(2, 2, 1, chroma.as_ptr()),
                PlaneSource::contiguous(2, 2, 1, chroma.as_ptr()),
            ]
        };

        let (pointers, _) = plane_arguments(&planes);
        // SAFETY: `luma.as_ptr()` plus one byte is still inside `luma`.
        assert_eq!(pointers[0], unsafe { luma.as_ptr().add(1) });
    }

    #[test]
    fn validation_matches_the_declared_chroma_geometry() {
        let colorspace = VshipColorspace::bt709_default(16, 16);
        let luma = [0u8; 16 * 16];
        let chroma = [0u8; 8 * 8];

        // SAFETY: each buffer covers exactly the geometry declared for its plane,
        // and the `PlaneSet` is consumed within this expression.
        let correct: PlaneSet = unsafe {
            [
                PlaneSource::contiguous(16, 16, 1, luma.as_ptr()),
                PlaneSource::contiguous(8, 8, 1, chroma.as_ptr()),
                PlaneSource::contiguous(8, 8, 1, chroma.as_ptr()),
            ]
        };
        assert!(validate_planes(&correct, &colorspace, "reference").is_ok());

        // The luma plane claims more width than the buffer holds, which the
        // validation must catch before the pointer reaches libvship.
        // SAFETY: this construction is never dereferenced, only validated.
        let wrong: PlaneSet = unsafe {
            [
                PlaneSource::contiguous(32, 16, 1, luma.as_ptr()),
                PlaneSource::contiguous(8, 8, 1, chroma.as_ptr()),
                PlaneSource::contiguous(8, 8, 1, chroma.as_ptr()),
            ]
        };
        assert!(matches!(
            validate_planes(&wrong, &colorspace, "reference"),
            Err(VshipError::InputMismatch { .. })
        ));
    }

    #[test]
    fn chroma_dimensions_follow_the_subsampling() {
        let mut colorspace = VshipColorspace::bt709_default(64, 32);

        assert_eq!(chroma_dimensions(&colorspace).unwrap(), (32, 16));
        colorspace.subsampling = crate::ffi::VshipChromaSubsample::yuv422();
        assert_eq!(chroma_dimensions(&colorspace).unwrap(), (32, 32));
        colorspace.subsampling = crate::ffi::VshipChromaSubsample::yuv444();
        assert_eq!(chroma_dimensions(&colorspace).unwrap(), (64, 32));

        colorspace.subsampling.subw = -1;
        assert!(chroma_dimensions(&colorspace).is_err());
    }

    #[test]
    fn frame_score_picks_the_metric_specific_value() {
        let score = FrameScore {
            score:    Some(0.9),
            norm_q:   Some(1.5),
            norm_3:   Some(2.0),
            norm_inf: Some(3.0),
        };

        assert_eq!(score.value(VshipMetric::Ssimulacra2), Some(0.9));
        assert_eq!(score.value(VshipMetric::Cvvdp), Some(0.9));
        assert_eq!(score.value(VshipMetric::Butteraugli), Some(1.5));
        assert_eq!(score.primary(), Some(0.9));
        assert_eq!(FrameScore::default().primary(), None);
    }

    #[test]
    fn pooling_uses_each_metrics_own_value() {
        let scores = [
            FrameScore {
                score: Some(0.9),
                norm_q: Some(1.0),
                ..FrameScore::default()
            },
            FrameScore {
                score: Some(0.7),
                norm_q: Some(3.0),
                ..FrameScore::default()
            },
        ];

        let mean = VshipScorer::pool(&scores, VshipMetric::Ssimulacra2, PoolMethod::Mean).unwrap();
        // Compared with a tolerance, since summing binary fractions rarely lands
        // exactly on 0.8.
        assert!((mean - 0.8).abs() < 1e-12, "mean was {mean}, expected 0.8");

        let mean = VshipScorer::pool(&scores, VshipMetric::Butteraugli, PoolMethod::Mean).unwrap();
        assert!((mean - 2.0).abs() < 1e-12, "mean was {mean}, expected 2.0");

        assert!(VshipScorer::pool(&[], VshipMetric::Cvvdp, PoolMethod::Mean).is_err());
    }

    /// A machine without libvship must be reported as unavailable rather than
    /// panicking, which is what keeps the suite green with no GPU and no
    /// binary.
    #[test]
    fn availability_is_total_and_never_panics() {
        match VshipScorer::is_available() {
            true => assert!(VshipScorer::libvship_version().is_some()),
            false => assert_eq!(VshipScorer::libvship_version(), None),
        }
        // These are `Option` or `Result` for the same reason.
        let _ = VshipScorer::device_name();
        let _ = VshipScorer::device_info();
    }
}
