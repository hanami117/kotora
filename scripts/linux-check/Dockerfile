# Linux check image: native Debian (arm64 on Apple silicon, amd64 elsewhere) with what install.sh needs
# to build and test the workspace headless. Nothing here runs the real app. Adapted from Uzumaki's.
FROM rust:1-trixie
RUN apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -yq --no-install-recommends \
      pkg-config build-essential git rsync ca-certificates \
      libdbus-1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libx11-dev libxcb1-dev libvulkan1 mesa-vulkan-drivers libegl1 libgl1-mesa-dri libgles2 \
      fontconfig fonts-dejavu-core fonts-noto-core \
 && rm -rf /var/lib/apt/lists/*
RUN rustup component add clippy
ENV HOME=/tmp/home CARGO_HOME=/cargo CARGO_TARGET_DIR=/target CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=line-tables-only
