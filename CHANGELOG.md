### 2
- Target VMAF renamed to Target Quality
- Changed Algo for Target Quality score calculation

### 3
- Default pix format to be yuv420p10le
- Default scene change interval to be 120 frames

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
- Fix concat on windows with ffmpeg
