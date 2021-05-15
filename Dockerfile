FROM luigi311/encoders-docker:latest

ENV MPLCONFIGDIR="/home/app_user/"
ARG DEBIAN_FRONTEND=noninteractive

# Install dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        mkvtoolnix && \
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

# Install av1an
COPY . /Av1an
WORKDIR /Av1an
RUN python3 setup.py install

# Change permissions
RUN chmod 777 -R /Av1an

USER app_user

VOLUME ["/videos"]
WORKDIR /videos

ENTRYPOINT [ "/Av1an/av1an.py" ]
