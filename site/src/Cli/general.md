
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
		Overwrite output file, without confirmation

-n
		Never overwrite output file, without confirmation

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

	--scaler <SCALER>
		Scaler used for scene detection (if --sc-downscale-height XXXX is used) and VMAF
        calculation

		Valid scalers are based on the scalers available in ffmpeg, including lanczos[1-9] with [1-9]
        defining the width of the lanczos scaler.

	--vspipe-args <VSPIPE_ARGS>
		Pass python argument(s) to the script environment

		Example: --vspipe-args "message=fluffy kittens" "head=empty"

-h, --help
		Print help information

-V, --version
		Print version information
```
