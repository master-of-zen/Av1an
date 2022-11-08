# Compiling Av1an

You can natively build Av1an on Linux and Windows. Cross-compilation is not supported.

## Compiling on Linux

To compile Av1an from source, you need the following dependencies:

- [Rust](https://www.rust-lang.org/) (version 1.63.0 or higher)
- [NASM](https://www.nasm.us/)
- [clang/LLVM](https://llvm.org/)
- [FFmpeg](https://ffmpeg.org/)
- [VapourSynth](https://www.vapoursynth.com/)

On Arch Linux, you can install these dependencies by running

```sh
pacman -S --needed rust nasm clang ffmpeg vapoursynth
```

Installation instructions on other distros will vary.

After installing the dependencies, you need to clone the repository and start the build process:

```sh
git clone https://github.com/master-of-zen/Av1an && cd Av1an
cargo build --release
```

The resulting binary will be the file `./target/release/av1an`.

## Compiling on Windows

If you just want a current build of Av1an that is newer than the last official release, you can find a pre-built binary of the current `master` branch at https://github.com/master-of-zen/Av1an/releases/tag/latest.

If you want to build the binary yourself, you will need the following dependencies:

- [Microsoft Visual C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) - this is a dependency for Rust
- [The Rust toolchain](https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe)
- [Python](https://www.python.org/) (version 3.8 or 3.10) - this is a dependency for VapourSynth. Recommended to install for all users
- [VapourSynth](https://github.com/vapoursynth/vapoursynth/releases/download/R58/VapourSynth64-R58.exe)
- [NASM](https://www.nasm.us/pub/nasm/releasebuilds/2.15.05/win64/nasm-2.15.05-installer-x64.exe)
- [FFmpeg](https://github.com/GyanD/codexffmpeg/releases/download/5.0.1/ffmpeg-5.0.1-full_build-shared.7z) (thanks to [gyan](https://github.com/GyanD) for providing these builds)

Extract the file `ffmpeg-5.0.1-full_build-shared.7z` to a directory, then create a new environment variable called `FFMPEG_DIR` (this can be done with with the "Edit environment variables for your account" function available in the control panel), and set it to the directory that you extracted the original file to (for example, set it to `C:\Users\Username\Downloads\ffmpeg-5.0.1-full_build-shared`).

Then, either clone the repository by running

```sh
git clone https://github.com/master-of-zen/Av1an
```

Or download and extract the [source code](https://github.com/master-of-zen/Av1an/archive/refs/heads/master.zip) manually.

Open a command prompt or PowerShell window inside the cloned repository/extracted ZIP folder and run the command `cargo build --release`. If this command executes successfully with no errors, `av1an.exe` will be in the folder `target\release`.

To use `av1an.exe`, copy all the `.dll` files from `ffmpeg-5.0.1-full_build-shared\bin` to the same directory as `av1an.exe`, and ensure that `ffmpeg.exe` is in a folder accessible via the `PATH` environment variable.
