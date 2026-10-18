# Docker

The [docker image](https://hub.docker.com/r/boatsmcgee/condor) is frequently updated and includes all supported encoders and all optional components. It is based on Arch Linux and provides recent versions of encoders and libraries, including the VSJET Pack VapourSynth plugins, Fast Film Grain Synthesis, and `libvship` and `fmetrics` built from source.

The image provides several kinds of tags that you can use:
- `boatsmcgee/condor:latest` for the most recent tagged release
- `boatsmcgee/condor:master` for the latest commit from `master`
- `boatsmcgee/condor:sha-#######` for a specific git commit (short hash)
- `boatsmcgee/condor:1`, `:1.2` and `:1.2.3` for a tagged release, by major, major.minor, or full version

## Examples

The image's entry point is `condor` and its working directory is `/videos`, so
the arguments you pass are `condor` arguments. The following examples assume the
file you want to encode is in your current working directory.

Linux

```bash
docker run --privileged -v "$(pwd):/videos" --user $(id -u):$(id -g) -it --rm boatsmcgee/condor:master -i S01E01.mkv -o S01E01.condor.mkv {options}
```

Windows

```powershell
docker run --privileged -v "${PWD}:/videos" -it --rm boatsmcgee/condor:master -i S01E01.mkv -o S01E01.condor.mkv {options}
```

Both `-i` and `-o` are given because a full run needs the input and the output
path; everything else comes from the configuration file or from the flags in
[condor](commands/condor.md).

The image can also be manually built by running

```bash
docker build -t "condor" .
```

in the root directory of this repository. The dependencies will automatically be installed into the image, no manual installations necessary.

To specify a different directory to use you would replace $(pwd) with the directory

```bash
docker run --privileged -v "/c/Users/me/Videos:/videos" --user $(id -u):$(id -g) -it --rm boatsmcgee/condor:master -i S01E01.mkv -o S01E01.condor.mkv {options}
```

The --user flag is required on linux to avoid permission issues with the docker container not being able to write to the location, if you get permission issues ensure your user has access to the folder that you are using to encode.