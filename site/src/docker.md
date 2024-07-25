# Docker

The [Docker image](https://hub.docker.com/r/masterofzen/av1an) is frequently updated and includes all supported encoders and all optional components. It is based on Arch Linux and provides recent versions of encoders and libraries.

Two tags are available for the Av1an image:
- `docker.io/masterofzen/av1an:master` for the latest commit to `master`
- `docker.io/masterofzen/av1an:sha-#######` for a specific git commit (short hash)

## Usage Examples

### Linux

The following example assumes the file(s) you wish to encode are within your current working directory.

```bash
docker run --privileged -v "$(pwd):/videos" --user $(id -u):$(id -g) -it --rm masterofzen/av1an:master -i input.mkv <options>
```
* The `--user` flag is required on linux to avoid permission issues with the docker container not being able to write to the location, if you get permission issues ensure your user has access to the folder that you are using to encode.

To simplify usage of the docker container, the above command can be aliased to a shorter command:
```bash
alias docker-av1an="docker run --privileged -v "$(pwd):/videos" --user $(id -u):$(id -g) -it --rm masterofzen/av1an:master"
```
Whereafter it can be quickly invoked more easily:
```bash
docker-av1an -i input.mkv
```

### Windows

The following examples assume the file you want to encode is in your current working directory.

```powershell
docker run --privileged -v "${PWD}:/videos" -it --rm masterofzen/av1an:master -i input.mkv <options>
```

## Building the Image

The Docker image can also be manually built by running:

```sh
docker build -t "av1an" .
```

-from this repository. The dependencies will automatically be installed into the image.

To specify a different directory to use you would replace `$(pwd)` with the directory:

```bash
docker run --privileged -v "/c/Users/masterofzen/Videos":/videos --user $(id -u):$(id -g) -it --rm masterofzen/av1an:master -i S01E01.mkv {options}
```