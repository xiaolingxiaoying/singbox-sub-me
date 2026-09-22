FROM rust:1-bookworm

# The same Linux GUI build dependencies CI installs (.github/workflows/ci.yml),
# plus a virtual display and a screenshotter so sbgui can be looked at without
# ever opening a window on a real desktop. Noto CJK matters: the UI is Chinese.
RUN apt-get update && apt-get install -y --no-install-recommends \
      libfontconfig1-dev libfreetype6-dev \
      libx11-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
      libgl1-mesa-dev libegl1-mesa-dev libvulkan-dev libasound2-dev \
      pkg-config libssl-dev ca-certificates \
      xvfb xauth x11-utils imagemagick fonts-noto-cjk fonts-noto-color-emoji \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
