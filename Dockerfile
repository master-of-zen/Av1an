FROM luigi311/encoders-docker:latest

ENV MPLCONFIGDIR="/home/app_user/"
ARG DEBIAN_FRONTEND=noninteractive
ARG DEPENDENCIES="mkvtoolnix curl python-is-python3 python3-venv"

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

# Copy av1an requirements
COPY --chown=app_user requirements.txt /Av1an/requirements.txt
WORKDIR /Av1an

# Create virtualenv required for maturin develop
ENV VIRTUAL_ENV=/Av1an/.venv
RUN python3 -m venv "${VIRTUAL_ENV}"
ENV PATH="$VIRTUAL_ENV/bin:/home/app_user/.cargo/bin:$PATH"

# Install av1an requirements
RUN pip3 install wheel && pip3 install -r requirements.txt vapoursynth

# Copy av1an and build av1an
COPY --chown=app_user . /Av1an
RUN maturin develop --release -m av1an-pyo3/Cargo.toml

# Open up /Av1an to all users
RUN chmod 777 /Av1an

VOLUME ["/videos"]
WORKDIR /videos

ENTRYPOINT [ "/Av1an/av1an.py" ]
