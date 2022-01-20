FROM archlinux:base-devel

ENV MPLCONFIGDIR="/home/app_user/"
ARG DEPENDENCIES="mkvtoolnix curl llvm clang"

# Create user

RUN pacman -Sy --noconfirm

# Install make dependencies
RUN pacman -S --noconfirm rust clang nasm

# Install all optional dependencies
RUN pacman -S --noconfirm aom ffmpeg vapoursynth ffms2 libvpx mkvtoolnix-cli rav1e svt-av1 vapoursynth-plugin-lsmashsource vmaf

RUN useradd -ms /bin/bash app_user
USER app_user

ENV PATH="/home/app_user/.cargo/bin:$PATH"

# Copy av1an and build av1an
COPY --chown=app_user . /Av1an
WORKDIR /Av1an
RUN cargo build --release

# Open up /Av1an to all users
RUN chmod 777 /Av1an

VOLUME ["/videos"]
WORKDIR /videos

ENTRYPOINT [ "/Av1an/target/release/av1an" ]
