FROM luigi311/encoders-docker:latest

ENV MPLCONFIGDIR="/home/app_user/"
ARG DEBIAN_FRONTEND=noninteractive
ARG DEPENDENCIES="mkvtoolnix curl"

# Install dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ${DEPENDENCIES} && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install VTM
RUN git clone https://vcgit.hhi.fraunhofer.de/jvet/VVCSoftware_VTM.git /VTM && \
    mkdir -p /VTM/build
WORKDIR /VTM/build
RUN cmake .. -DCMAKE_BUILD_TYPE=Release && \
    make -j"$(nproc)" && \
    ln -s ../bin/EncoderAppStatic /usr/local/bin/vvc_encoder

# Create user
RUN useradd -ms /bin/bash app_user
USER app_user

# Install rust
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y --default-toolchain nightly
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
