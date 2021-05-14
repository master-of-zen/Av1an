### 6
- ~1.7x faster probes for svt-av1
- ~2x faster probes for aomenc
-  Changed rav1e settings, disable rav1e scene detection
- Temporally removed VVC support until it's 100% ready and working
- Added overwrite promt


### 5
- Fixed fatal errors with ffms2/lsmash
- Added vmaf validation on each time when VMAF initialized
- Fix running not required frame probe
- Chunk restarting
- Fixed ffmpeg segmenting
- `color_range 0` by default for pipes
- aomenc target quality probes to be 8 bit

### 4
- Refactored Args to Project class
- Removed dead Rust code
- Default encoder settings changed
- Better Vapousynth error handling
- Target Quality settings balanced
- Target Quality score calculation fixed and improved
- Default extra_splits set to 240 frames.
- Extraction and Concatenation to copy all streams.
- Scenes file to save total frame count, faster restart/start up with scenes file.
- Fix concat on windows with ffmpeg.
- Revorked args to classes.
- Skip files in queue if they already encoded.
- Default chunk method to ffms2.
- Per frame target quality for SVT-AV1
- Added none split method option
- Added webm output
- Refactored VMAF and Target Quality
- VMAF is now separate, ready for import module
- Changed Target Quality probing rate
- Refactored module structure/names

### 3
- Default pix format to be yuv420p10le
- Default scene change interval to be 120 frames

### 2
- Target VMAF renamed to Target Quality
- Changed Algo for Target Quality score calculation