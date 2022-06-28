FROM archlinux:base-devel AS base

RUN pacman -Syyu --noconfirm

# Install dependancies needed by all steps including runtime step
RUN pacman -S --noconfirm aom ffmpeg vapoursynth ffms2 libvpx mkvtoolnix-cli svt-av1 vapoursynth-plugin-lsmashsource vmaf


FROM base AS build-base

# Install dependancies needed by build steps
RUN pacman -S --noconfirm rust clang nasm git 

# TODO reimplement caching via cargo-chef after it properly fixes its --profile support
# RUN cargo install cargo-chef
WORKDIR /tmp/Av1an


FROM build-base AS planner

# See previous TODO
# COPY . .
# RUN cargo chef prepare


FROM build-base AS cacher

# See previous TODO
# COPY --from=planner /tmp/Av1an/recipe.json recipe.json
# RUN cargo chef cook --release


FROM build-base AS build

# Compile rav1e from git, as archlinux is still on rav1e 0.4
RUN git clone https://github.com/xiph/rav1e && \
    cd rav1e && \
    cargo build --release && \
    strip ./target/release/rav1e && \
    mv ./target/release/rav1e /usr/local/bin && \
    cd .. && rm -rf ./rav1e

# Build av1an
COPY . /tmp/Av1an

# See previous TODO
# Copy over the cached dependencies
# COPY --from=cacher /tmp/Av1an/target /tmp/Av1an/target

RUN cargo build --release && \
    mv ./target/release/av1an /usr/local/bin && \
    cd .. && rm -rf ./Av1an


FROM base AS runtime

ENV MPLCONFIGDIR="/home/app_user/"

COPY --from=build /usr/local/bin/rav1e /usr/local/bin/rav1e
COPY --from=build /usr/local/bin/av1an /usr/local/bin/av1an

# Create user
RUN useradd -ms /bin/bash app_user
USER app_user

VOLUME ["/videos"]
WORKDIR /videos

ENTRYPOINT [ "/usr/local/bin/av1an" ]
