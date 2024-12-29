# Target Quality

```
	--target-quality <TARGET_QUALITY>
		Target a VMAF score for encoding (disabled by default)

		For each task, target quality uses an algorithm to find the quantizer/crf needed to
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
		early exits and min_q is used for the task.

		If not specified, the default value is used (chosen per encoder).

	--max-q <MAX_Q>
		Upper bound for target quality Q-search early exit

		If max_q is tested and the probe's VMAF score is higher than target_quality, the Q-
		search early exits and max_q is used for the task.

		If not specified, the default value is used (chosen per encoder).
```