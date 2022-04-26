#![allow(clippy::inline_always)]

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct NoiseGenArgs {
  pub iso_setting: u32,
  pub width: u32,
  pub height: u32,
  pub transfer_function: TransferFunction,
}

const NUM_Y_POINTS: usize = 14;
type ScalingPoints = [[u8; 2]; NUM_Y_POINTS];

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy)]
pub enum TransferFunction {
  /// For SDR content
  BT1886,
  /// For HDR content
  SMPTE2084,
}

const PQ_M1: f32 = 2610. / 16384.;
const PQ_M2: f32 = 128. * 2523. / 4096.;
const PQ_C1: f32 = 3424. / 4096.;
const PQ_C2: f32 = 32. * 2413. / 4096.;
const PQ_C3: f32 = 32. * 2392. / 4096.;

const BT1886_WHITEPOINT: f32 = 203.;
const BT1886_BLACKPOINT: f32 = 0.1;
const BT1886_GAMMA: f32 = 2.4;

// BT.1886 formula from https://en.wikipedia.org/wiki/ITU-R_BT.1886.
//
// TODO: the inverses, alpha, and beta should all be constants
// once floats in const fns are stabilized and `powf` is const.
// Until then, `inline(always)` gets us close enough.

#[inline(always)]
fn bt1886_inv_whitepoint() -> f32 {
  BT1886_WHITEPOINT.powf(1.0 / BT1886_GAMMA)
}

#[inline(always)]
fn bt1886_inv_blackpoint() -> f32 {
  BT1886_BLACKPOINT.powf(1.0 / BT1886_GAMMA)
}

/// The variable for user gain:
/// `α = (Lw^(1/λ) - Lb^(1/λ)) ^ λ`
#[inline(always)]
fn bt1886_alpha() -> f32 {
  (bt1886_inv_whitepoint() - bt1886_inv_blackpoint()).powf(BT1886_GAMMA)
}

/// The variable for user black level lift:
/// `β = Lb^(1/λ) / (Lw^(1/λ) - Lb^(1/λ))`
#[inline(always)]
fn bt1886_beta() -> f32 {
  bt1886_inv_blackpoint() / (bt1886_inv_whitepoint() - bt1886_inv_blackpoint())
}

impl TransferFunction {
  pub fn to_linear(self, x: f32) -> f32 {
    match self {
      TransferFunction::BT1886 => {
        // The screen luminance in cd/m^2:
        // L = α * max((x + β, 0))^λ
        let luma = bt1886_alpha() * 0f32.max(x + bt1886_beta()).powf(BT1886_GAMMA);

        // Normalize to between 0.0 and 1.0
        luma / BT1886_WHITEPOINT
      }
      TransferFunction::SMPTE2084 => {
        let pq_pow_inv_m2 = x.powf(1. / PQ_M2);
        (0_f32.max(pq_pow_inv_m2 - PQ_C1) / (PQ_C2 - PQ_C3 * pq_pow_inv_m2)).powf(1. / PQ_M1)
      }
    }
  }

  #[allow(clippy::wrong_self_convention)]
  pub fn from_linear(self, x: f32) -> f32 {
    match self {
      TransferFunction::BT1886 => {
        // Scale to a raw cd/m^2 value
        let luma = x * BT1886_WHITEPOINT;

        // The inverse of the `to_linear` formula:
        // `(L / α)^(1 / λ) - β = x`
        (luma / bt1886_alpha()).powf(1.0 / BT1886_GAMMA) - bt1886_beta()
      }
      TransferFunction::SMPTE2084 => {
        if x < f32::EPSILON {
          return 0.0;
        }
        let linear_pow_m1 = x.powf(PQ_M1);
        (PQ_C2.mul_add(linear_pow_m1, PQ_C1) / PQ_C3.mul_add(linear_pow_m1, 1.)).powf(PQ_M2)
      }
    }
  }

  #[inline(always)]
  pub fn mid_tone(self) -> f32 {
    self.to_linear(0.5)
  }
}

fn generate_photon_noise(args: NoiseGenArgs) -> ScalingPoints {
  // Assumes a daylight-like spectrum.
  // https://www.strollswithmydog.com/effective-quantum-efficiency-of-sensor/#:~:text=11%2C260%20photons/um%5E2/lx-s
  const PHOTONS_PER_SQ_MICRON_PER_LUX_SECOND: f32 = 11260.;

  // Order of magnitude for cameras in the 2010-2020 decade, taking the CFA into account.
  const EFFECTIVE_QUANTUM_EFFICIENCY: f32 = 0.2;

  // Also reasonable values for current cameras. The read noise is typically
  // higher than this at low ISO settings but it matters less there.
  const PHOTO_RESPONSE_NON_UNIFORMITY: f32 = 0.005;
  const INPUT_REFERRED_READ_NOISE: f32 = 1.5;

  // Focal plane exposure for a mid-tone (typically a 18% reflectance card), in lx·s.
  let mid_tone_exposure = 10. / args.iso_setting as f32;

  // Assumes a 35mm sensor (36mm × 24mm).
  const SENSOR_AREA: f32 = 36_000. * 24_000.;
  let pixel_area_microns = SENSOR_AREA / (args.width * args.height) as f32;

  let mid_tone_electrons_per_pixel = EFFECTIVE_QUANTUM_EFFICIENCY
    * PHOTONS_PER_SQ_MICRON_PER_LUX_SECOND
    * mid_tone_exposure
    * pixel_area_microns;
  let max_electrons_per_pixel = mid_tone_electrons_per_pixel / args.transfer_function.mid_tone();

  let mut scaling_points = ScalingPoints::default();
  for (i, point) in scaling_points.iter_mut().enumerate() {
    let x = i as f32 / (NUM_Y_POINTS as f32 - 1.);
    let linear = args.transfer_function.to_linear(x);
    let electrons_per_pixel = max_electrons_per_pixel * linear;

    // Quadrature sum of the relevant sources of noise, in electrons rms. Photon
    // shot noise is sqrt(electrons) so we can skip the square root and the
    // squaring.
    // https://en.wikipedia.org/wiki/Addition_in_quadrature
    // https://doi.org/10.1117/3.725073
    let noise_in_electrons =
      (PHOTO_RESPONSE_NON_UNIFORMITY * PHOTO_RESPONSE_NON_UNIFORMITY * electrons_per_pixel)
        .mul_add(
          electrons_per_pixel,
          INPUT_REFERRED_READ_NOISE.mul_add(INPUT_REFERRED_READ_NOISE, electrons_per_pixel),
        )
        .sqrt();
    let linear_noise = noise_in_electrons / max_electrons_per_pixel;
    let linear_range_start = 0_f32.max(linear - 2. * linear_noise);
    let linear_range_end = 1_f32.min(2_f32.mul_add(linear_noise, linear));
    let tf_slope = (args.transfer_function.from_linear(linear_range_end)
      - args.transfer_function.from_linear(linear_range_start))
      / (linear_range_end - linear_range_start);
    let encoded_noise = linear_noise * tf_slope;

    let x = (255. * x).round() as u8;
    let encoded_noise = 255_f32.min((255. * 7.88 * encoded_noise).round()) as u8;

    point[0] = x;
    point[1] = encoded_noise;
  }

  scaling_points
}

pub fn create_film_grain_file(
  filename: &Path,
  strength: u8,
  width: u32,
  height: u32,
  transfer: TransferFunction,
) -> anyhow::Result<()> {
  let params = generate_photon_noise(NoiseGenArgs {
    iso_setting: u32::from(strength) * 100,
    width,
    height,
    transfer_function: transfer,
  });
  let mut file = BufWriter::new(File::create(filename)?);
  write_film_grain_table(params, &mut file)
}

fn write_film_grain_table(
  scaling_points: ScalingPoints,
  file: &mut BufWriter<File>,
) -> anyhow::Result<()> {
  writeln!(file, "filmgrn1")?;
  writeln!(file, "E 0 {} 1 7391 1", i64::MAX)?;
  writeln!(file, "\tp 0 6 0 8 0 1 0 0 0 0 0 0")?;
  write!(file, "\tsY {} ", NUM_Y_POINTS)?;
  for point in &scaling_points {
    write!(file, " {} {}", point[0], point[1])?;
  }
  writeln!(file)?;
  writeln!(file, "\tsCb 0")?;
  writeln!(file, "\tsCr 0")?;
  writeln!(file, "\tcY")?;
  writeln!(file, "\tcCb 0")?;
  writeln!(file, "\tcCr 0")?;
  file.flush()?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use quickcheck::TestResult;
  use quickcheck_macros::quickcheck;

  #[quickcheck]
  fn bt1886_to_linear_within_range(x: f32) -> TestResult {
    if x < 0.0 || x > 1.0 || x.is_nan() {
      return TestResult::discard();
    }

    let tx = TransferFunction::BT1886;
    let res = tx.to_linear(x);
    TestResult::from_bool(res >= 0.0 && res <= 1.0)
  }

  #[quickcheck]
  fn bt1886_to_linear_reverts_correctly(x: f32) -> TestResult {
    if x < 0.0 || x > 1.0 || x.is_nan() {
      return TestResult::discard();
    }

    let tx = TransferFunction::BT1886;
    let res = tx.to_linear(x);
    let res = tx.from_linear(res);
    TestResult::from_bool((x - res).abs() < f32::EPSILON)
  }

  #[quickcheck]
  fn smpte2084_to_linear_within_range(x: f32) -> TestResult {
    if x < 0.0 || x > 1.0 || x.is_nan() {
      return TestResult::discard();
    }

    let tx = TransferFunction::SMPTE2084;
    let res = tx.to_linear(x);
    TestResult::from_bool(res >= 0.0 && res <= 1.0)
  }

  #[quickcheck]
  fn smpte2084_to_linear_reverts_correctly(x: f32) -> TestResult {
    if x < 0.0 || x > 1.0 || x.is_nan() {
      return TestResult::discard();
    }

    let tx = TransferFunction::SMPTE2084;
    let res = tx.to_linear(x);
    let res = tx.from_linear(res);
    TestResult::from_bool((x - res).abs() < f32::EPSILON)
  }
}
