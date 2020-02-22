FROM ubuntu:latest

ARG DEBIAN_FRONTEND=noninteractive
#Missing libgstreamer ugly plugins
RUN apt-get update && apt-get -y install --no-install-recommends  build-essential \
  curl \
  ca-certificates \
 libasound2-dev  \
 libavcodec-dev \
  libavformat-dev \
  libswscale-dev \
  libgstreamer1.0-dev \
   libgstreamer-plugins-base1.0-dev \
  libgstreamer-plugins-good1.0-dev \
  libgstreamer-plugins-bad1.0-dev \
        gstreamer1.0-plugins-base \
         gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad \
         gstreamer1.0-plugins-ugly \
        gstreamer1.0-libav \
         libgstrtspserver-1.0-dev \
         pkg-config \
         gstreamer1.0-tools \
         pulseaudio \
         libssl-dev \
         git \
         make

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    RUST_VERSION=1.48.0 \
    PKG_CONFIG_PATH=/usr/lib/pkgconfig;/usr/local/lib/pkgconfig \
    AWS_SHARED_CREDENTIALS_FILE=/.aws/credentials
RUN ldconfig -v
RUN curl https://sh.rustup.rs -fsS | bash -s -- -y



WORKDIR /s3multiframesink/
COPY ./Cargo.toml .
COPY ./Cargo.lock .
RUN mkdir .cargo
RUN cargo vendor > .cargo/config
COPY build.rs .
COPY Makefile .
COPY deja_vu.mp4 .
COPY ./.aws/credentials /.aws/credentials
COPY ./src src
ENV GST_PLUGIN_PATH=/s3multiframesink/target/release:$GST_PLUGIN_PATH
RUN cargo build --release
RUN make install
RUN gst-inspect-1.0 ./target/release/libs3multiframesink.so
CMD ["/bin/bash"]
