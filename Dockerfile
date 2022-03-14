FROM archlinux:base-devel AS planner
RUN pacman -Syy --noconfirm

# Install all dependencies (except for rav1e)
RUN pacman -S --noconfirm rsync rust clang nasm git aom ffmpeg vapoursynth ffms2 libvpx mkvtoolnix-cli svt-av1 vapoursynth-plugin-lsmashsource vmaf

WORKDIR /tmp/Av1an
RUN cargo install cargo-chef 
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json




FROM archlinux:base-devel AS cacher
RUN pacman -Syy --noconfirm

# Install all dependencies (except for rav1e)
RUN pacman -S --noconfirm rsync rust clang nasm git aom ffmpeg vapoursynth ffms2 libvpx mkvtoolnix-cli svt-av1 vapoursynth-plugin-lsmashsource vmaf

WORKDIR /tmp/Av1an
RUN cargo install cargo-chef
COPY --from=planner /tmp/Av1an/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json




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

# Build av1an
COPY . /tmp/Av1an

# Copy over the cached dependencies
COPY --from=cacher /tmp/Av1an/target /tmp/Av1an/target

WORKDIR /tmp/Av1an
RUN cargo build --release
RUN mv ./target/release/av1an /usr/local/bin



FROM archlinux:base-devel AS runtime

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
