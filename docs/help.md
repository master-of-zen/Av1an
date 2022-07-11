# Av1an CLI reference

`av1an [OPTIONS] -i <INPUT>`

---

## Available options

- [General](#general)
- [Scene detection](#scene-detection)
- [Encoding](#encoding)
- [VMAF](#vmaf)
- [Target Quality](#target-quality)

---

## General

```
-i <INPUT>
		Input file to encode
		
		Can be a video or vapoursynth (.py, .vpy) script.

-o <OUTPUT_FILE>
		Video output file

	--temp <TEMP>
		Temporary directory to use
		
		If not specified, the temporary directory name is a hash of the input file name.

-q, --quiet
		Disable printing progress to the terminal

	--verbose
		Print extra progress info and stats to terminal

-l, --log-file <LOG_FILE>
		Log file location [default: <temp dir>/log.log]

	--log-level <LOG_LEVEL>
		Set log level for log file (does not affect command-line log level)
		
		error: Designates very serious errors.
		
		warn: Designates hazardous situations.
		
		info: Designates useful information.
		
		debug: Designates lower priority information.
		
		trace: Designates very low priority, often extremely verbose, information. Includes
		rav1e scenechange decision info.
		
		[default: DEBUG]
		[possible values: error, warn, info, debug, trace]

-r, --resume
		Resume previous session from temporary directory

-k, --keep
		Do not delete the temporary folder after encoding has finished

	--force
		Do not check if the encoder arguments specified by -v/--video-params are valid

-y
		Overwrite output file without confirmation

	--max-tries <MAX_TRIES>
		Maximum number of chunk restarts for an encode
		
		[default: 3]

-w, --workers <WORKERS>
		Number of workers to spawn [0 = automatic]
		
		[default: 0]

	--set-thread-affinity <SET_THREAD_AFFINITY>
		Pin each worker to a specific set of threads of this size (disabled by default)
		
		This is currently only supported on Linux and Windows, and does nothing on unsupported
		platforms. Leaving this option unspecified allows the OS to schedule all processes
		spawned.

-h, --help
		Print help information

-V, --version
		Print version information
```

## Scene detection

```
-s, --scenes <SCENES>
		File location for scenes

	--split-method <SPLIT_METHOD>
		Method used to determine chunk boundaries
		
		"av-scenechange" uses an algorithm to analyze which frames of the video are the start
		of new scenes, while "none" disables scene detection entirely (and only relies on
		-x/--extra-split to add extra scenecuts).
		
		[default: av-scenechange]
		[possible values: av-scenechange, none]

	--sc-method <SC_METHOD>
		Scene detection algorithm to use for av-scenechange
		
		Standard: Most accurate, still reasonably fast. Uses a cost-based algorithm to determine
		keyframes.
		
		Fast: Very fast, but less accurate. Determines keyframes based on the raw difference
		between pixels.
		
		[default: standard]
		[possible values: standard, fast]

	--sc-only
		Run the scene detection only before exiting
		
		Requires a scene file with --scenes.

	--sc-pix-format <SC_PIX_FORMAT>
		Perform scene detection with this pixel format

	--sc-downscale-height <SC_DOWNSCALE_HEIGHT>
		Optional downscaling for scene detection
		
		Specify as the desired maximum height to scale to (e.g. "720" to downscale to 720p
		— this will leave lower resolution content untouched). Downscaling improves scene
		detection speed but lowers accuracy, especially when scaling to very low resolutions.
		
		By default, no downscaling is performed.

-x, --extra-split <EXTRA_SPLIT>
		Maximum scene length
		
		When a scenecut is found whose distance to the previous scenecut is greater than the
		value specified by this option, one or more extra splits (scenecuts) are added. Set this
		option to 0 to disable adding extra splits.

	--min-scene-len <MIN_SCENE_LEN>
		Minimum number of frames for a scenecut
		
		[default: 24]
```

## Encoding

```
-e, --encoder <ENCODER>
		Video encoder to use
		
		[default: aom]
		[possible values: aom, rav1e, vpx, svt-av1, x264, x265]

-v, --video-params <VIDEO_PARAMS>
		Parameters for video encoder
		
		These parameters are for the encoder binary directly, so the ffmpeg syntax cannot be
		used. For example, CRF is specified in ffmpeg via "-crf <crf>", but the x264 binary
		takes this value with double dashes, as in "--crf <crf>". See the --help output of each
		encoder for a list of valid options.

-p, --passes <PASSES>
		Number of encoder passes
		
		Since aom and vpx benefit from two-pass mode even with constant quality mode (unlike
		other encoders in which two-pass mode is used for more accurate VBR rate control), two-
		pass mode is used by default for these encoders.
		
		When using aom or vpx with RT mode (--rt), one-pass mode is always used regardless
		of the value specified by this flag (as RT mode in aom and vpx only supports one-pass
		encoding).
		
		[possible values: 1, 2]

-a, --audio-params <AUDIO_PARAMS>
		Audio encoding parameters (ffmpeg syntax)
		
		If not specified, "-c:a copy" is used.
		
		Do not use ffmpeg's -map syntax with this option. Instead, use the colon syntax with
		each parameter you specify.
		
		Subtitles are always copied by default.
		
		Example to encode all audio tracks with libopus at 128k:
		
		-a="-c:a libopus -b:a 128k"
		
		Example to encode the first audio track with libopus at 128k, and the second audio track
		with aac at 24k, where only the second track is downmixed to a single channel:
		
		-a="-c:a:0 libopus -b:a:0 128k -c:a:1 aac -ac:a:1 1 -b:a:1 24k"

-f, --ffmpeg <FFMPEG_FILTER_ARGS>
		FFmpeg filter options

-m, --chunk-method <CHUNK_METHOD>
		Method used for piping exact ranges of frames to the encoder
		
		Methods that require an external vapoursynth plugin:
		
		lsmash - Generally the best and most accurate method. Does not require intermediate
		files. Errors generally only occur if the input file itself is broken (for example,
		if the video bitstream is invalid in some way, video players usually try to recover
		from the errors as much as possible even if it results in visible artifacts, while
		lsmash will instead throw an error). Requires the lsmashsource vapoursynth plugin to
		be installed.
		
		ffms2 - Accurate and does not require intermediate files. Can sometimes have bizarre
		bugs that are not present in lsmash (that can cause artifacts in the piped output).
		Slightly faster than lsmash for y4m input. Requires the ffms2 vapoursynth plugin to
		be installed.
		
		Methods that only require ffmpeg:
		
		hybrid - Uses a combination of segment and select. Usually accurate but requires
		intermediate files (which can be large). Avoids decoding irrelevant frames by seeking to
		the first keyframe before the requested frame and decoding only a (usually very small)
		number of irrelevant frames until relevant frames are decoded and piped to the encoder.
		
		select - Extremely slow, but accurate. Does not require intermediate files. Decodes
		from the first frame to the requested frame, without skipping irrelevant frames (causing
		quadratic decoding complexity).
		
		segment - Create chunks based on keyframes in the source. Not frame exact, as it can
		only split on keyframes in the source. Requires intermediate files (which can be large).
		
		Default: lsmash (if available), otherwise ffms2 (if available), otherwise hybrid.
		
		[possible values: segment, select, ffms2, lsmash, hybrid]

	--chunk-order <CHUNK_ORDER>
		The order in which av1an will encode chunks
		
		Available methods:
		
		long-to-short - The longest chunks will be encoded first. This method results in the
		smallest amount of time with idle cores, as the encode will not be waiting on a very
		long chunk to finish at the end of the encode after all other chunks have finished.
		
		short-to-long - The shortest chunks will be encoded first.
		
		sequential - The chunks will be encoded in the order they appear in the video.
		
		random - The chunks will be encoded in a random order. This will provide a more accurate
		estimated filesize sooner in the encode.
		
		[default: long-to-short]
		[possible values: long-to-short, short-to-long, sequential, random]

	--photon-noise <PHOTON_NOISE>
		Generates a photon noise table and applies it using grain synthesis [strength: 0-64]
		(disabled by default)
		
		Photon noise tables are more visually pleasing than the film grain generated by aomenc,
		and provide a consistent level of grain regardless of the level of grain in the source.
		Strength values correlate to ISO values, e.g. 1 = ISO 100, and 64 = ISO 6400. This
		option currently only supports aomenc.
		
		An encoder's grain synthesis will still work without using this option, by specifying
		the correct parameter to the encoder. However, the two should not be used together, and
		specifying this option will disable aomenc's internal grain synthesis.

-c, --concat <CONCAT>
		Determines method used for concatenating encoded chunks and audio into output file
		
		ffmpeg - Uses ffmpeg for concatenation. Unfortunately, ffmpeg sometimes produces files
		with partially broken audio seeking, so mkvmerge should generally be preferred if
		available. ffmpeg concatenation also produces broken files with the --enable-keyframe-
		filtering=2 option in aomenc, so it is disabled if that option is used. However, ffmpeg
		can mux into formats other than matroska (.mkv), such as WebM. To output WebM, use
		a .webm extension in the output file.
		
		mkvmerge - Generally the best concatenation method (as it does not have either of the
		aforementioned issues that ffmpeg has), but can only produce matroska (.mkv) files.
		Requires mkvmerge to be installed.
		
		ivf - Experimental concatenation method implemented in av1an itself to concatenate to an
		ivf file (which only supports VP8, VP9, and AV1, and does not support audio).
		
		[default: ffmpeg]
		[possible values: ffmpeg, mkvmerge, ivf]

	--pix-format <PIX_FORMAT>
		FFmpeg pixel format
		
		[default: yuv420p10le]

	--zones <ZONES>
		Path to a file specifying zones within the video with differing encoder settings.
		
		The zones file should include one zone per line,
		with each arg within a zone space-separated.
		No quotes or escaping are needed around the encoder args,
		as these are assumed to be the last argument.
		
		The zone args on each line should be in this order:
		
		```
		start_frame end_frame encoder reset(opt) video_params
		```
		
		For example:
		
		```
		136 169 aom --photon-noise 4 --cq-level=32
		169 1330 rav1e reset -s 3 -q 42
		```
		
		Example line 1 will encode frames 136-168 using aomenc
		with the argument `--cq-level=32` and enable av1an's `--photon-noise` option.
		Note that the end frame number is *exclusive*.
		The start and end frame will both be forced to be scenecuts.
		Additional scene detection will still be applied within the zones.
		`-1` can be used to refer to the last frame in the video.
		
		The default behavior as shown on line 1 is to preserve
		any options passed to `--video-params` or `--photon-noise`
		in av1an, and append or overwrite the additional zone settings.
		
		Example line 2 will encode frames 169-1329 using rav1e.
		The `reset` keyword instructs av1an to ignore any settings
		which affect the encoder, and use only the parameters from this zone.
		
		For segments where no zone is specified,
		the settings passed to av1an itself will be used.
		
		The video params which may be specified include any parameters
		that are allowed by the encoder, as well as the following av1an options:
		
		- `-x`/`--extra-split`
		- `--min-scene-len`
		- `--passes`
		- `--photon-noise` (aomenc only)
```

## VMAF

```
	--vmaf
		Plot an SVG of the VMAF for the encode
		
		This option is independent of --target-quality, i.e. it can be used with or without it.
		The SVG plot is created in the same directory as the output file.

	--vmaf-path <VMAF_PATH>
		Path to VMAF model (used by --vmaf and --target-quality)
		
		If not specified, ffmpeg's default is used.

	--vmaf-res <VMAF_RES>
		Resolution used for VMAF calculation
		
		[default: 1920x1080]

	--vmaf-threads <VMAF_THREADS>
		Number of threads to use for VMAF calculation

	--vmaf-filter <VMAF_FILTER>
		Filter applied to source at VMAF calcualation
		
		This option should be specified if the source is cropped, for example.
```

## Target Quality

```
	--target-quality <TARGET_QUALITY>
		Target a VMAF score for encoding (disabled by default)
		
		For each chunk, target quality uses an algorithm to find the quantizer/crf needed to
		achieve a certain VMAF score. Target quality mode is much slower than normal encoding,
		but can improve the consistency of quality in some cases.
		
		The VMAF score range is 0-100 (where 0 is the worst quality, and 100 is the best).
		Floating-point values are allowed.

	--probes <PROBES>
		Maximum number of probes allowed for target quality
		
		[default: 4]

	--probing-rate <PROBING_RATE>
		Framerate for probes, 1 - original
		
		[default: 1]

	--probe-slow
		Use encoding settings for probes specified by --video-params rather than faster, less
		accurate settings
		
		Note that this always performs encoding in one-pass mode, regardless of --passes.

	--min-q <MIN_Q>
		Lower bound for target quality Q-search early exit
		
		If min_q is tested and the probe's VMAF score is lower than target_quality, the Q-search
		early exits and min_q is used for the chunk.
		
		If not specified, the default value is used (chosen per encoder).

	--max-q <MAX_Q>
		Upper bound for target quality Q-search early exit
		
		If max_q is tested and the probe's VMAF score is higher than target_quality, the Q-
		search early exits and max_q is used for the chunk.
		
		If not specified, the default value is used (chosen per encoder).
```