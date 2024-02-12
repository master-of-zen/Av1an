
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
		Maximum scene length, in frames

		When a scenecut is found whose distance to the previous scenecut is greater than the
		value specified by this option, one or more extra splits (scenecuts) are added. Set this
		option to 0 to disable adding extra splits.

    --extra-split-sec <EXTRA_SPLIT_SEC>
		Maximum scene length, in seconds

        If both frames and seconds are specified, then the number of frames will take priority.

		[default: 10]

	--min-scene-len <MIN_SCENE_LEN>
		Minimum number of frames for a scenecut

		[default: 24]

    --ignore-frame-mismatch
        Ignore any detected mismatch between scene frame count and encoder frame count
```
