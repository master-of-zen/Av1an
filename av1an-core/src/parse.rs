//! Functions for parsing frames from encoder output.
//!
//! Some functions are optimized with SIMD, and need
//! runtime detection that the corresponding feature
//! set is available before calling them.

use std::borrow::Cow;
use std::collections::HashSet;

use crate::encoder::Encoder;

// We can safely always ignore this prefix, as the second number will
// always be at some point after this prefix. See examples of aomenc
// output below to see why this is the case.
#[rustfmt::skip]
const AOM_VPX_IGNORED_PREFIX: &str =
  "Pass x/x frame    x/";
// Pass 1/1 frame    3/2       2131B    5997 us 500.25 fps [ETA  unknown]
//                     ^ relevant output starts at this character
// Pass 1/1 frame   84/83     81091B  132314 us 634.85 fps [ETA  unknown]
//                     ^
// Pass 1/1 frame  142/141   156465B  208875 us 679.83 fps [ETA  unknown]
//                     ^
// Pass 1/1 frame 4232/4231 5622510B 5518075 us 766.93 fps [ETA  unknown]
//                     ^
// Pass 1/1 frame 13380/13379 17860525B   16760 ms 798.31 fps [ETA  unknown]
//                      ^
// Pass 1/1 frame 102262/102261 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F
//                       ^
// Pass 1/1 frame 1022621/1022611 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F
//                        ^
//
// As you can see, the relevant part of the output always starts past
// the length of the ignored prefix.

pub fn parse_aom_vpx_frames(s: &str) -> Option<u64> {
  // The numbers for aomenc/vpxenc are buffered/encoded frames, so we want the
  // second number (actual encoded frames)
  let first_digit_index = s
    .as_bytes()
    .get(AOM_VPX_IGNORED_PREFIX.len() - 1..)?
    .iter()
    .position(|&c| c == b'/')?;

  let first_space_index = s
    .get(AOM_VPX_IGNORED_PREFIX.len() + first_digit_index..)?
    .as_bytes()
    .iter()
    .position(|&c| c == b' ')?
    + first_digit_index;

  s.get(
    AOM_VPX_IGNORED_PREFIX.len() + first_digit_index
      ..AOM_VPX_IGNORED_PREFIX.len() + first_space_index,
  )?
  .parse()
  .ok()
}

/// x86 SIMD implementation of parsing aomenc/vpxenc output using
/// SSSE3+SSE4.1, returning the number of frames processed, or `None`
/// if the input did not match.
///
/// This function also works for parsing vpxenc output, as its progress
/// printing is exactly the same.
///
/// # Safety
///
/// The CPU must support SSSE3 and SSE4.1.
#[inline]
#[target_feature(enable = "ssse3,sse4.1")]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub unsafe fn parse_aom_vpx_frames_sse41(s: &[u8]) -> Option<u64> {
  #[cfg(target_arch = "x86")]
  use std::arch::x86::*;
  #[cfg(target_arch = "x86_64")]
  use std::arch::x86_64::*;
  use std::mem::transmute;

  // This implementation matches the *second* number in the output. Ex:
  // Pass 1/1 frame  142/141   156465B  208875 us 679.83 fps [ETA  unknown]
  //                     ^^^
  //                     matches this number and returns `Some(141)`
  //
  // Pass 1/1 frame 102262/102261 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F
  //                       ^^^^^^
  //                       matches this number and returns `Some(102261)`
  //
  // If invalid input is detected, this function returns `None`.
  // We cheat in this implementation by taking a mutable slice to the string
  // so we can reuse its allocation to add padding zeroes for free.

  // Number of bytes processed (size in bytes of xmm register)
  //
  // There is no benefit to using wider SIMD lanes in this case, so we just
  // use the most commonly available SIMD width. This is because we want
  // to parse the fewest number of bytes possible to get the correct result.
  const CHUNK_SIZE: usize = 16;

  // This implementation needs to read `CHUNK_SIZE` bytes past the ignored
  // prefix, so we pay the cost of the bounds check only once at the start
  // of this function. This also serves as an input validation check.
  if s.len() < AOM_VPX_IGNORED_PREFIX.len() + CHUNK_SIZE {
    return None;
  }

  // Since the aomenc output follows a particular pattern, we can calculate the
  // position of the '/' character from the index of the first space (how to
  // do so is explained later on). We create this mask to find the first space
  // in the output.
  let spaces = _mm_set1_epi8(b' ' as i8);

  // Load the relevant part of the output, which are the 16 bytes after the ignored prefix.
  // This is safe because we already asserted that at least `IGNORED_PREFIX.len() + CHUNK_SIZE`
  // bytes are available, and `_mm_loadu_si128` loads `CHUNK_SIZE` (16) bytes.
  let relevant_output = _mm_loadu_si128(
    s.get_unchecked(AOM_VPX_IGNORED_PREFIX.len()..)
      .as_ptr()
      .cast(),
  );

  // Compare the relevant output to spaces to create a mask where each bit
  // is set to 1 if the corresponding character was a space, and 0 otherwise.
  // The LSB corresponds to the match between the first characters.
  //
  // Only the lower 16 bits are relevant, as the rest are always set to 0.
  let mask16 = _mm_movemask_epi8(_mm_cmpeq_epi8(relevant_output, spaces));

  // The bits in the mask are set as so:
  //
  //       "141   156465B  208875 us 679.83 fps [ETA  unknown]"
  // mask:  110000000111000
  //                    ^^^
  //                    These bits correspond to the first 3 characters: "141".
  //                    Since they do not match the space, they are set to 0 in the mask.
  //                    As printed, the leftmost bit is the most significant bit.
  //                 ^^^
  //                 These bits correspond to the 3 spaces after the "141".
  //
  //       "2/102261 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F"
  // mask:  100000000
  //         ^^^^^^^^
  //         These bits correspond to the first 8 characters: "2/102261".
  //
  // To get the index of the first space, we need to get the trailing zeros,
  // which correspond to the first characters.
  //
  // This value is such that `relevant_output[first_space_index]` gives the
  // actual first space character.
  let first_space_index = mask16.trailing_zeros() as usize;

  let first_digit_index = {
    _mm_movemask_epi8(_mm_cmpeq_epi8(
      _mm_loadu_si128(s.get(AOM_VPX_IGNORED_PREFIX.len() - 1..)?.as_ptr().cast()),
      _mm_set1_epi8(b'/' as i8),
    ))
    .trailing_zeros() as usize
  };

  let ascending = _mm_set_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
  let num_digits = match first_space_index.checked_sub(first_digit_index) {
    Some(nd) => nd,
    None => return None,
  };

  let dynamic_mask = _mm_cmplt_epi8(ascending, _mm_set1_epi8(num_digits as i8));

  // At this point, we have done all the setup and can use the actual SIMD integer
  // parsing algorithm. The description of the algorithm can be found here:
  // https://kholdstare.github.io/technical/2020/05/26/faster-integer-parsing.html
  let mut chunk = _mm_loadu_si128(
    s.as_ptr()
      .add(AOM_VPX_IGNORED_PREFIX.len() + first_space_index - CHUNK_SIZE)
      .cast(),
  );

  let zeros = _mm_set1_epi8(b'0' as i8);
  chunk = _mm_sub_epi8(chunk, zeros);

  // Mask out the irrelevant bits, effectively parsing them as if they were 0.
  chunk = _mm_and_si128(chunk, dynamic_mask);

  let mult = _mm_set_epi8(1, 10, 1, 10, 1, 10, 1, 10, 1, 10, 1, 10, 1, 10, 1, 10);
  chunk = _mm_maddubs_epi16(chunk, mult);
  let mult = _mm_set_epi16(1, 100, 1, 100, 1, 100, 1, 100);
  chunk = _mm_madd_epi16(chunk, mult);
  chunk = _mm_packus_epi32(chunk, chunk);
  let mult = _mm_set_epi16(0, 0, 0, 0, 1, 10000, 1, 10000);
  chunk = _mm_madd_epi16(chunk, mult);

  let chunk = transmute::<_, [u64; 2]>(chunk);

  Some(((chunk[0] & 0xffff_ffff) * 100_000_000) + (chunk[0] >> 32))
}

pub fn parse_rav1e_frames(s: &str) -> Option<u64> {
  #[rustfmt::skip]
  const RAV1E_IGNORED_PREFIX: &str =
    "encoded ";
  // encoded 1 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s
  // encoded 12 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s
  // encoded 122 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s
  // encoded 1220 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s
  // encoded 12207 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s
  // encoded 12/240 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s

  if !s.starts_with(RAV1E_IGNORED_PREFIX) {
    return None;
  }

  s.get(RAV1E_IGNORED_PREFIX.len()..)?
    .split_ascii_whitespace()
    .next()
    .map(|val| val.split_once('/').map_or(val, |(val, _)| val))
    .and_then(|s| s.parse().ok())
}

pub fn parse_svt_av1_frames(s: &str) -> Option<u64> {
  const SVT_AV1_IGNORED_PREFIX: &str = "Encoding frame";

  if !s.starts_with(SVT_AV1_IGNORED_PREFIX) {
    return None;
  }

  s.get(SVT_AV1_IGNORED_PREFIX.len()..)?
    .split_ascii_whitespace()
    .next()
    .and_then(|s| s.parse().ok())
}

pub fn parse_x26x_frames(s: &str) -> Option<u64> {
  s.split_ascii_whitespace()
    .find(|part| !part.starts_with('['))
    .map(|val| val.split_once('/').map_or(val, |(val, _)| val))
    .and_then(|s| s.parse().ok())
}

/// Returns the set of valid parameters given a help text for the given encoder
#[must_use]
pub fn valid_params(help_text: &str, encoder: Encoder) -> HashSet<Cow<'_, str>> {
  // x265 has 292 parameters, which is the most of any encoder, so we round up
  // slightly just in case
  let mut params = HashSet::with_capacity(300);

  for s in help_text.split_ascii_whitespace() {
    if s.starts_with('-') {
      if s.len() == 1 || s == "--" {
        continue;
      }

      if encoder == Encoder::x265 {
        // x265 does this: -m/--subme
        //        or even: -w/--[no-]weightp
        // So we need to ensure that in this case the short parameter is also handled.
        let s = s.get("-x/".len()..).map_or(s, |stripped| {
          if stripped.starts_with("--") {
            params.insert(Cow::Borrowed(&s[..2]));

            stripped
          } else {
            s
          }
        });

        // Somehow x265 manages to have a buggy --help, where a single option (--[no-]-hrd-concat)
        // has an extra dash.
        let arg = s
          .strip_prefix("--[no-]")
          .map(|stripped| stripped.strip_prefix('-').unwrap_or(stripped));

        if let Some(arg) = arg {
          params.insert(Cow::Owned(format!("--{arg}")));
          params.insert(Cow::Owned(format!("--no-{arg}")));
          continue;
        }
      }

      // aomenc outputs '--tune=<arg>' for example, so we have to find the character
      // from the left so as to not miss the leftmost char
      if let Some(idx) = s.find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_') {
        // In some weird cases (like with x264) there may be a dash followed by a non alphanumeric
        // character, so we just ignore that.
        if idx > 1 {
          params.insert(Cow::Borrowed(&s[..idx]));
        }
      } else {
        // It's a little concerning how *two* encoders manage to have buggy help output.
        let arg = if encoder == Encoder::vpx {
          // vpxenc randomly truncates the "Vizier Rate Control Options" in the help
          // output, which sometimes causes it to truncate at a dash, which breaks the
          // tests if we don't do this. Not sure what the correct solution in this case is.
          s.strip_suffix('-').unwrap_or(s)
        } else {
          s
        };

        params.insert(Cow::Borrowed(arg));
      }
    }
  }

  params
}

#[cfg(test)]
mod tests {
  use crate::parse::*;

  #[test]
  #[allow(clippy::cognitive_complexity)]
  fn valid_params_works() {
    use std::borrow::Borrow;

    macro_rules! generate_tests {
      ($($x:ident),* $(,)?) => {
        $(
          let returned: HashSet<String> = valid_params(include_str!(concat!("../tests/", stringify!($x), "_help.txt")), Encoder::$x)
            .iter()
            .map(|s| s.to_string())
            .collect();
          let expected: HashSet<String> = include_str!(concat!("../tests/", stringify!($x), "_params.txt"))
            .split_ascii_whitespace()
            .map(|s| s.to_string())
            .collect();

          for arg in expected.iter() {
            assert!(returned.contains(Borrow::<str>::borrow(&**arg)), "expected '{}', but it was missing in return value (for encoder {})", arg, stringify!($x));
          }
          for arg in returned.iter() {
            assert!(expected.contains(Borrow::<str>::borrow(&**arg)), "return value contains '{}', but it was not expected (for encoder {})", arg, stringify!($x));
          }
          assert_eq!(returned, expected);
        )*
      };
    }

    generate_tests!(aom, rav1e, svt_av1, vpx, x264, x265);
  }

  #[test]
  fn rav1e_parsing() {
    let test_cases = [
      (
        "encoded 1 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s",
        Some(1),
      ),
      (
        "encoded 12 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s",
        Some(12),
      ),
      (
        "encoded 122 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s",
        Some(122),
      ),
      (
        "encoded 1220 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s",
        Some(1220),
      ),
      (
        "encoded 12207 frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s",
        Some(12207),
      ),
      ("invalid", None),
      (
        "encoded xxxx frames, 126.416 fps, 16.32 Kb/s, elap. time: 1m 36s",
        None,
      ),
    ];

    for (s, ans) in test_cases {
      assert_eq!(parse_rav1e_frames(s), ans);
    }
  }

  #[test]
  fn x26x_parsing() {
    let test_cases = [
      ("24 frames: 39.11 fps, 14.60 kb/s", Some(24)),
      ("240 frames: 39.11 fps, 14.60 kb/s", Some(240)),
      ("2445 frames: 39.11 fps, 14.60 kb/s", Some(2445)),
      ("24145 frames: 39.11 fps, 14.60 kb/s", Some(24145)),
      ("246434 frames: 39.11 fps, 14.60 kb/s", Some(246_434)),
      ("2448732 frames: 39.11 fps, 14.60 kb/s", Some(2_448_732)),
      (
        "[42.5%] 121/285 frames, 2.47 fps, 1445.28 kb/s, eta 0:01:06",
        Some(121),
      ),
      ("invalid data", None),
      ("", None),
    ];

    for (s, ans) in test_cases {
      assert_eq!(parse_x26x_frames(s), ans);
    }
  }

  #[test]
  fn svt_av1_parsing() {
    let test_cases = [
      ("Encoding frame    0 1.08 kbps 2.00 fps", Some(0)),
      ("Encoding frame    7 1.08 kbps 2.00 fps", Some(7)),
      ("Encoding frame   22 2.03 kbps 3.68 fps", Some(22)),
      ("Encoding frame  7654 1.08 kbps 2.00 fps", Some(7654)),
      ("Encoding frame 72415 1.08 kbps 2.00 fps", Some(72415)),
      ("Encoding frame 778743 1.08 kbps 2.00 fps", Some(778_743)),
      (
        "Encoding frame 53298734 1.08 kbps 2.00 fps",
        Some(53_298_734),
      ),
      ("invalid input", None),
      ("", None),
    ];

    for (s, ans) in test_cases {
      assert_eq!(parse_svt_av1_frames(s), ans);
    }
  }

  #[test]
  #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
  fn aom_vpx_parsing() {
    // We return the number of frames without checking
    // if those frames are not the final pass.

    // Therefore, parse_encoded_frames shouldn't be called
    // when running the first out of two passes.
    let test_cases = [
      (
        "Pass 1/1 frame    3/2       2131B    5997 us 500.25 fps [ETA  unknown]",
        Some(2),
      ),
      (
        "Pass 2/2 frame   84/83     81091B  132314 us 634.85 fps [ETA  unknown]",
        Some(83),
      ),
      (
        "Pass 1/1 frame  142/141   156465B  208875 us 679.83 fps [ETA  unknown]",
        Some(141),
      ),
      (
        "Pass 1/2 frame  142/141   156465B  208875 us 679.83 fps [ETA  unknown]",
        Some(141),
      ),
      (
        "Pass 2/2 frame 4232/4231 5622510B 5518075 us 766.93 fps [ETA  unknown]",
        Some(4231),
      ),
      (
        "Pass 1/1 frame 13380/13379 17860525B   16760 ms 798.31 fps [ETA  unknown]",
        Some(13379),
      ),
      (
        "Pass 1/2 frame 13380/13379 17860525B   16760 ms 798.31 fps [ETA  unknown]",
        Some(13379),
      ),
      ("invalid data", None),
      (
        "Pass 2/2 frame 102262/102261 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F",
        Some(102_261),
      ),
      (
        "Pass 1/1 frame 1022621/1022611 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F",
        Some(1_022_611),
      ),
      (
        "Pass 1/1 frame 10000/9999 5319227B 8663074 us 1154.32 fps [ETA  unknown]",
        Some(9999),
      ),
    ];

    if is_x86_feature_detected!("sse4.1") && is_x86_feature_detected!("ssse3") {
      for (s, ans) in test_cases {
        assert_eq!(unsafe { parse_aom_vpx_frames_sse41(s.as_bytes()) }, ans);
      }
    }

    for (s, ans) in test_cases {
      assert_eq!(parse_aom_vpx_frames(s), ans);
    }
  }
}
