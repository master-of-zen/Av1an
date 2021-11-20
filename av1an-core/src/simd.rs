//! SIMD-optimized functions for parsing

/// x86 SIMD implementation of parsing aomenc/vpxenc output using
/// SSSE3+SSE4.1, returning the number of frames processed, or `None`
/// if the input did not match.
///
/// This function also works for parsing vpxenc output, as its progress
/// printing is exactly the same.
///
/// # Safety
///
/// The caller should not attempt to read the contents of `s` after this
/// function has been called.
#[inline]
#[target_feature(enable = "ssse3,sse4.1")]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) unsafe fn parse_aom_vpx_frames_sse41(s: &mut [u8]) -> Option<u64> {
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

  // We can safely always ignore this prefix, as the second number will
  // always be at some point after this prefix. See examples of aomenc
  // output below to see why this is the case.
  #[rustfmt::skip]
  const IGNORED_PREFIX: &str =
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

  // This implementation needs to read `CHUNK_SIZE` bytes past the ignored
  // prefix, so we pay the cost of the bounds check only once at the start
  // of this function. This also serves as an input validation check.
  if s.len() < IGNORED_PREFIX.len() + CHUNK_SIZE {
    return None;
  }
  // Sanity check to see if we should parse the line. Processing invalid input
  // anyway would result in returning a garbage value, ultimately causing the
  // frame counter to be completely off.
  if !(s.starts_with(b"Pass 2/2") || s.starts_with(b"Pass 1/1")) {
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
  let relevant_output =
    _mm_loadu_si128(s.get_unchecked(IGNORED_PREFIX.len()..).as_ptr() as *const _);

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

  // It is impossible that `first_space_index == 0` for valid aomenc output, since the
  // first character after the ignored prefix has to be a digit.
  //
  //                       All indexes are relative to `relevant_output`.
  //                     ↓ Since the first digit occurs here, its index = 0
  // Pass 1/1 frame    3/2       2131B    5997 us 500.25 fps [ETA  unknown]
  //                    * ^ n = 1, first_digit_index = 0 (the first character is the first digit)
  //                    ↑
  //                    ╰ end of ignored prefix (continued below)
  //
  // Pass 1/1 frame   84/83     81091B  132314 us 634.85 fps [ETA  unknown]
  //                    *  ^ n = 2, first_digit_index = 0
  //
  // Pass 1/1 frame  142/141   156465B  208875 us 679.83 fps [ETA  unknown]
  //                    *   ^ n = 3, first_digit_index = 0
  //
  // Pass 1/1 frame 4232/4231 5622510B 5518075 us 766.93 fps [ETA  unknown]
  //                    *    ^ n = 4, first_digit_index = 0
  //
  // Pass 1/1 frame 13380/13379 17860525B   16760 ms 798.31 fps [ETA  unknown]
  //                    *      ^ n = 6, first_digit_index = 1
  //
  // Pass 1/1 frame 102262/102261 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F
  //                    *        ^ n = 8, first_digit_index = 2
  //                     ^^^^^^^^
  //                     12345678
  //                     n = 8 signifies that there are 8 characters before the first space.
  //                     This also happens to be first_space_index.
  //
  // Pass 1/1 frame 1022621/1022611 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F
  //                    *          ^ n = 10, first_digit_index = 3
  //
  // Solving a linear equation for n and first_digit_index yields this
  // formula, but first_digit_index cannot be negative so we use saturating_sub.
  let first_digit_index = (first_space_index / 2).saturating_sub(2);

  // Set `CHUNK_SIZE` bytes before the real first digit index (including the ignored prefix)
  // to b'0'. Uncoditionally zeroing `CHUNK_SIZE` bytes is better than only setting the
  // bytes that are absolutely necessary because using a fixed size allows LLVM to avoid
  // a call to memset and instead use movaps/movups.
  for byte in s
    .get_unchecked_mut(IGNORED_PREFIX.len() + first_digit_index - CHUNK_SIZE..)
    .get_unchecked_mut(..CHUNK_SIZE)
  {
    *byte = b'0';
  }

  // At this point, we have done all the setup and can use the actual SIMD integer
  // parsing algorithm. The description of the algorithm can be found here:
  // https://kholdstare.github.io/technical/2020/05/26/faster-integer-parsing.html
  let mut chunk = _mm_loadu_si128(
    s.as_ptr()
      .add(IGNORED_PREFIX.len() + first_space_index - CHUNK_SIZE) as *const _,
  );

  let zeros = _mm_set1_epi8(b'0' as i8);
  chunk = _mm_sub_epi8(chunk, zeros);

  let mult = _mm_set_epi8(1, 10, 1, 10, 1, 10, 1, 10, 1, 10, 1, 10, 1, 10, 1, 10);
  chunk = _mm_maddubs_epi16(chunk, mult);
  let mult = _mm_set_epi16(1, 100, 1, 100, 1, 100, 1, 100);
  chunk = _mm_madd_epi16(chunk, mult);
  chunk = _mm_packus_epi32(chunk, chunk);
  let mult = _mm_set_epi16(0, 0, 0, 0, 1, 10000, 1, 10000);
  chunk = _mm_madd_epi16(chunk, mult);

  let chunk = transmute::<_, [u64; 2]>(chunk);

  Some(((chunk[0] & 0xffffffff) * 100000000) + (chunk[0] >> 32))
}

#[cfg(test)]
mod tests {
  use crate::simd::parse_aom_vpx_frames_sse41;

  #[test]
  #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
  fn aom_vpx_sse41_parsing() {
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
        None,
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
        None,
      ),
      ("invalid data", None),
      (
        "Pass 2/2 frame 102262/102261 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F",
        Some(102261),
      ),
      (
        "Pass 1/1 frame 1022621/1022611 136473850B  131502 ms 777.65 fps [ETA  unknown]    1272F",
        Some(1022611),
      ),
    ];

    if is_x86_feature_detected!("sse4.1") {
      for (s, ans) in test_cases {
        let mut s = String::from(s);

        assert_eq!(unsafe { parse_aom_vpx_frames_sse41(s.as_bytes_mut()) }, ans);
      }
    }
  }
}
