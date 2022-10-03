FROM archlinux:base-devel AS base

RUN pacman -Syu --noconfirm

# Install dependancies needed by all steps including runtime step
RUN pacman -Syu --noconfirm --needed ffmpeg vapoursynth ffms2 libvpx mkvtoolnix-cli vapoursynth-plugin-lsmashsource vmaf


FROM base AS build-base

# Install dependancies needed by build steps
RUN pacman -Syu --noconfirm --needed rust clang nasm git yasm cmake numactl wget meson ninja

RUN cargo install cargo-chef
WORKDIR /tmp/Av1an


FROM build-base AS planner

COPY . .
RUN cargo chef prepare


FROM build-base AS build

COPY --from=planner /tmp/Av1an/recipe.json recipe.json
RUN cargo chef cook --release

# Compile rav1e from git, as archlinux is still on rav1e 0.4
RUN git clone https://github.com/xiph/rav1e && \
    cd rav1e && \
    cargo build --release && \
    strip ./target/release/rav1e && \
    mv ./target/release/rav1e /usr/local/bin && \
    cd .. && rm -rf ./rav1e

# bump: x264 /X264_VERSION=([[:xdigit:]]+)/ gitrefs:https://bitbucket.org/multicoreware/x265_git.git|re:#^refs/heads/master$#|@commit
# bump: x265 after ./hashupdate Dockerfile X265 $LATEST
# bump: x265 link "Source diff $CURRENT..$LATEST" https://bitbucket.org/multicoreware/x265_git/branches/compare/$LATEST..$CURRENT#diff
ARG X265_VERSION=931178347b3f73e40798fd5180209654536bbaa5
ARG X265_URL="https://bitbucket.org/multicoreware/x265_git/get/${X265_VERSION}.tar.bz2"
# -w-macro-params-legacy to not log lots of asm warnings
# https://bitbucket.org/multicoreware/x265_git/issues/559/warnings-when-assembling-with-nasm-215
RUN \
  curl -Ssl -o x265_git.tar.bz2 "$X265_URL" && \
  tar xf x265_git.tar.bz2 && \
  cd multicoreware-x265_git-*/build/linux && \
  sed -i '/^cmake / s/$/ -G "Unix Makefiles" ${CMAKEFLAGS}/' ./multilib.sh && \
  sed -i 's/ -DENABLE_SHARED=OFF//g' ./multilib.sh && \
  sed -i 's/set(ARM_ARGS -fPIC -flax-vector-conversions)/set(ARM_ARGS -DPIC -fPIC -flax-vector-conversions)/' ../../source/CMakeLists.txt && \
  MAKEFLAGS="-j$(nproc)" \
  CMAKEFLAGS="-DENABLE_SHARED=OFF -DCMAKE_VERBOSE_MAKEFILE=ON -DENABLE_AGGRESSIVE_CHECKS=ON -DCMAKE_ASM_NASM_FLAGS=-w-macro-params-legacy -DENABLE_NASM=ON -DCMAKE_BUILD_TYPE=Release" \
  ./multilib.sh && \
  make -C 8bit -j$(nproc) install && \
  cd .. && rm -rf ./x265_git.tar.bz2 && rm -rf ./multicoreware-x265_git-*

# before aom as libvmaf uses it
# bump: vmaf /VMAF_VERSION=([\d.]+)/ https://github.com/Netflix/vmaf.git|*
# bump: vmaf after ./hashupdate Dockerfile VMAF $LATEST
# bump: vmaf link "Release" https://github.com/Netflix/vmaf/releases/tag/v$LATEST
# bump: vmaf link "Source diff $CURRENT..$LATEST" https://github.com/Netflix/vmaf/compare/v$CURRENT..v$LATEST
ARG VMAF_VERSION=2.3.1
ARG VMAF_URL="https://github.com/Netflix/vmaf/archive/refs/tags/v$VMAF_VERSION.tar.gz"
ARG VMAF_SHA256=8d60b1ddab043ada25ff11ced821da6e0c37fd7730dd81c24f1fc12be7293ef2
RUN \
  wget $WGET_OPTS -O vmaf.tar.gz "$VMAF_URL" && \
  echo "$VMAF_SHA256  vmaf.tar.gz" | sha256sum --status -c - && \
  tar xf vmaf.tar.gz && \
  cd vmaf-*/libvmaf && meson build --buildtype=release -Ddefault_library=static -Dbuilt_in_models=true -Denable_tests=false -Denable_docs=false -Denable_avx512=true -Denable_float=true && \
  ninja -j$(nproc) -vC build install && \
  cd ../.. && rm -rf vmaf.tar.gz vmaf-*
# extra libs stdc++ is for vmaf https://github.com/Netflix/vmaf/issues/788
RUN  sed -i 's/-lvmaf /-lvmaf -lstdc++ /' /usr/local/lib/pkgconfig/libvmaf.pc

# build after libvmaf
# bump: aom /AOM_VERSION=([\d.]+)/ git:https://aomedia.googlesource.com/aom|*
# bump: aom after ./hashupdate Dockerfile AOM $LATEST
# bump: aom after COMMIT=$(git ls-remote https://aomedia.googlesource.com/aom v$LATEST^{} | awk '{print $1}') && sed -i -E "s/^ARG AOM_COMMIT=.*/ARG AOM_COMMIT=$COMMIT/" Dockerfile
# bump: aom link "CHANGELOG" https://aomedia.googlesource.com/aom/+/refs/tags/v$LATEST/CHANGELOG
ARG AOM_VERSION=3.5.0
ARG AOM_URL="https://aomedia.googlesource.com/aom"
ARG AOM_COMMIT=bcfe6fbfed315f83ee8a95465c654ee8078dbff9
RUN \
  git clone --depth 1 --branch v$AOM_VERSION "$AOM_URL" && \
  cd aom && test $(git rev-parse HEAD) = $AOM_COMMIT && \
  mkdir build_tmp && cd build_tmp && \
  cmake \
    -G"Unix Makefiles" \
    -DCMAKE_VERBOSE_MAKEFILE=ON \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_SHARED_LIBS=OFF \
    -DENABLE_EXAMPLES=1 \
    -DCONFIG_AV1_ENCODER=1 \
    -DCONFIG_AV1_DECODER=1 \
    -DENABLE_DOCS=NO \
    -DENABLE_TESTS=NO \
    -DCONFIG_TUNE_VMAF=1 \
    -DENABLE_NASM=ON \
    -DCMAKE_INSTALL_LIBDIR=lib \
    .. && \
  make -j$(nproc) install && \
  cd ../.. && rm -rf aom


# bump: svtav1 /SVTAV1_VERSION=([\d.]+)/ https://gitlab.com/AOMediaCodec/SVT-AV1.git|*
# bump: svtav1 after ./hashupdate Dockerfile SVTAV1 $LATEST
# bump: svtav1 link "Release notes" https://gitlab.com/AOMediaCodec/SVT-AV1/-/releases/v$LATEST
ARG SVTAV1_VERSION=1.2.1
ARG SVTAV1_URL="https://gitlab.com/AOMediaCodec/SVT-AV1/-/archive/v$SVTAV1_VERSION/SVT-AV1-v$SVTAV1_VERSION.tar.bz2"
ARG SVTAV1_SHA256=805827daa8aedec4f1362b959f377075e2a811680bfc76b6f4fbf2ef4e7101d4
RUN \
  wget $WGET_OPTS -O svtav1.tar.bz2 "$SVTAV1_URL" && \
  echo "$SVTAV1_SHA256  svtav1.tar.bz2" | sha256sum --status -c - && \
  tar xf svtav1.tar.bz2 && \
  cd SVT-AV1-*/Build && \
  cmake \
    -G"Unix Makefiles" \
    -DCMAKE_VERBOSE_MAKEFILE=ON \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DBUILD_SHARED_LIBS=OFF \
    -DNATIVE=ON \
    -DENABLE_AVX512=ON \
    -DCMAKE_BUILD_TYPE=Release \
    .. && \
  make -j$(nproc) install


# Build av1an
COPY . /tmp/Av1an

RUN cargo build --release && \
    mv ./target/release/av1an /usr/local/bin && \
    cd .. && rm -rf ./Av1an


FROM base AS runtime

ENV MPLCONFIGDIR="/home/app_user/"
RUN pacman -Syu --noconfirm --needed numactl

COPY --from=mwader/static-ffmpeg:5.1.2 /ffmpeg /usr/local/bin/
COPY --from=mwader/static-ffmpeg:5.1.2 /ffprobe /usr/local/bin/
COPY --from=build /usr/local/bin/rav1e /usr/local/bin/rav1e
COPY --from=build /usr/local/bin/x265 /usr/sbin/x265
COPY --from=build /usr/local/bin/aomenc /usr/sbin/aomenc
COPY --from=build /usr/local/bin/aomdec /usr/sbin/aomdec
COPY --from=build /usr/local/bin/SvtAv1EncApp /usr/sbin/SvtAv1EncApp
COPY --from=build /usr/local/bin/SvtAv1DecApp /usr/sbin/SvtAv1DecApp
COPY --from=build /usr/local/bin/av1an /usr/local/bin/av1an
RUN ln -s /usr/share/model /usr/local/share/model

# Create user
RUN useradd -ms /bin/bash app_user
USER app_user

VOLUME ["/videos"]
WORKDIR /videos

ENTRYPOINT [ "/usr/local/bin/av1an" ]
