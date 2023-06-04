# Encoding

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
		encoder for a list of valid options. This list of parameters will be merged into
		Av1an's default set of encoder parameters, except if --force is set.

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

        dgdecnv - Very fast, but only decodes AVC, HEVC, MPEG-2, and VC1. Does not require intermediate files.
	    Requires dgindexnv to be present in system path, NVIDIA GPU that support CUDA video decoding, and dgdecnv vapoursynth plugin
        to be installed.

	    bestsource - Very slow but accurate. Linearly decodes input files. Does not require intermediate files, requires the BestSource vapoursynth plugin
        to be installed.

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

		Default: lsmash (if available), otherwise ffms2 (if available), otherwise DGDecNV (if available), otherwise bestsource (if available), otherwise hybrid.

		[possible values: segment, select, ffms2, lsmash, dgdecnv, bestsource, hybrid]

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
		option currently only supports aomenc and rav1e.

		An encoder's grain synthesis will still work without using this option, by specifying
		the correct parameter to the encoder. However, the two should not be used together, and
		specifying this option will disable the encoder's internal grain synthesis.

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
		- `--photon-noise` (aomenc/rav1e only)
```
