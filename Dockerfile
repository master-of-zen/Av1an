FROM archlinux:base-devel AS build

RUN pacman -Syy --noconfirm

# Install all dependencies (except for rav1e)
RUN pacman -S --noconfirm rsync rust clang nasm git aom ffmpeg vapoursynth ffms2 libvpx mkvtoolnix-cli svt-av1 vapoursynth-plugin-lsmashsource vmaf

# Compile rav1e from git, as archlinux is still on rav1e 0.4
RUN git clone https://github.com/xiph/rav1e /tmp/rav1e
WORKDIR /tmp/rav1e
RUN cargo build --release && \
    strip ./target/release/rav1e 
RUN mv ./target/release/rav1e /usr/local/bin

# Build only dependencies to speed up subsequent builds
RUN cargo new /tmp/av1an-deps
COPY Cargo.toml Cargo.lock /tmp/av1an-deps/
COPY av1an-cli/Cargo.toml /tmp/av1an-deps/av1an-cli/
COPY av1an-core/Cargo.toml /tmp/av1an-deps/av1an-core/
WORKDIR /tmp/av1an-deps
RUN for d in /tmp/av1an-deps/av1an-* ; do cp -R /tmp/av1an-deps/src "$d"/; done && \
    cargo build --release

# Build av1an
COPY . /tmp/Av1an
WORKDIR /tmp/Av1an
RUN cargo build --release
RUN mv ./target/release/av1an /usr/local/bin



FROM archlinux:base-devel

ENV MPLCONFIGDIR="/home/app_user/"

RUN pacman -Syy --noconfirm

# Install all optional dependencies (except for rav1e)
RUN pacman -S --noconfirm aom ffmpeg vapoursynth ffms2 libvpx mkvtoolnix-cli svt-av1 vapoursynth-plugin-lsmashsource vmaf

COPY --from=build /usr/local/bin/rav1e /usr/local/bin/rav1e
COPY --from=build /usr/local/bin/av1an /usr/local/bin/av1an

# Create user
RUN useradd -ms /bin/bash app_user
USER app_user

VOLUME ["/videos"]
WORKDIR /videos

ENTRYPOINT [ "/usr/local/bin/av1an" ]
