# Av1an in Docker

The [docker image](https://hub.docker.com/r/masterofzen/av1an) is frequently updated and includes all supported encoders and all optional components. It is based on Arch Linux and provides recent versions of encoders and libraries.

The image provides three types of tags that you can use:
- `masterofzen/av1an:master` for the latest commit from `master`
- `masterofzen/av1an:sha-#######` for a specific git commit (short hash)

## Examples

The following examples assume the file you want to encode is in your current working directory.

Linux

```bash
docker run --privileged -v "$(pwd):/videos" --user $(id -u):$(id -g) -it --rm masterofzen/av1an:master -i S01E01.mkv {options}
```

Windows

```powershell
docker run --privileged -v "${PWD}:/videos" -it --rm masterofzen/av1an:master -i S01E01.mkv {options}
```

The image can also be manually built by running

```sh
docker build -t "av1an" .
```

in the root directory of this repository. The dependencies will automatically be installed into the image, no manual installations necessary.

To specify a different directory to use you would replace $(pwd) with the directory

```bash
docker run --privileged -v "/c/Users/masterofzen/Videos":/videos --user $(id -u):$(id -g) -it --rm masterofzen/av1an:master -i S01E01.mkv {options}
```

The --user flag is required on linux to avoid permission issues with the docker container not being able to write to the location, if you get permission issues ensure your user has access to the folder that you are using to encode.