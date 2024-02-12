
# VMAF

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

        If set to inputres, the output video will be scaled to the resolution of the input video.

		[default: 1920x1080]

	--vmaf-threads <VMAF_THREADS>
		Number of threads to use for VMAF calculation

	--vmaf-filter <VMAF_FILTER>
		Filter applied to source at VMAF calcualation

		This option should be specified if the source is cropped, for example.
```