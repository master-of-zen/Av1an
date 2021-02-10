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
RUN git clone https://vcgit.hhi.fraunhofer.de/jvet/VVCSoftware_VTM.git /home/app_user/VTM && \
    mkdir -p /home/app_user/VTM/build
WORKDIR /home/app_user/VTM/build
RUN cmake .. -DCMAKE_BUILD_TYPE=Release && \
    make -j"$(nproc)" && \
    ln -s ../bin/EncoderAppStatic /usr/local/bin/vvc_encoder

# Install av1an
COPY . /home/app_user/Av1an
WORKDIR /home/app_user/Av1an
RUN python3 setup.py install

# Change permissions
RUN chmod 777 -R /home/app_user

USER app_user

VOLUME ["/videos"]
WORKDIR /videos

ENTRYPOINT [ "/home/app_user/Av1an/av1an.py" ]
