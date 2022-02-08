FROM archlinux:base-devel

ENV MPLCONFIGDIR="/home/app_user/"
ARG DEPENDENCIES="mkvtoolnix curl llvm clang"

RUN pacman -Syy --noconfirm

# Install make dependencies
RUN pacman -S --noconfirm rust clang nasm git

# Install all optional dependencies (except for rav1e)
RUN pacman -S --noconfirm aom ffmpeg vapoursynth ffms2 libvpx mkvtoolnix-cli svt-av1 vapoursynth-plugin-lsmashsource vmaf

# Compile rav1e from git, as archlinux is still on rav1e 0.4
RUN git clone https://github.com/xiph/rav1e && \
    cd rav1e && \
    cargo build --release && \
    strip ./target/release/rav1e && \
    mv ./target/release/rav1e /usr/local/bin && \
    cd .. && rm -rf ./rav1e

# Create user
RUN useradd -ms /bin/bash app_user

# Copy av1an and build av1an
COPY --chown=app_user . /Av1an
WORKDIR /Av1an
RUN cargo build --release && \
    mv ./target/release/av1an /usr/local/bin && \
    cd .. && rm -rf ./Av1an

# Remove build dependencies
RUN pacman -R --noconfirm rust clang nasm git
USER app_user

VOLUME ["/videos"]
WORKDIR /videos

ENTRYPOINT [ "/usr/local/bin/av1an" ]
