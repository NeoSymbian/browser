# SPDX-License-Identifier: Apache-2.0
# WB0 cross-build image: Debian trixie (arm64 on the Mac) building Servo for armv7-unknown-linux-gnueabihf.
# The Rust version must match Servo v0.8.0's rust-toolchain.toml.
FROM debian:trixie

ARG RUST_VERSION=1.99.1

RUN dpkg --add-architecture armhf && apt-get update && apt-get install -y --no-install-recommends \
      build-essential git curl ca-certificates python3 python3-venv python3-pip m4 perl pkg-config \
      cmake ninja-build clang-19 libclang-19-dev llvm-19-dev \
      gcc-arm-linux-gnueabihf g++-arm-linux-gnueabihf \
    && apt-get install -y --no-install-recommends \
      libc6-dev:armhf libstdc++-14-dev:armhf libfontconfig1-dev:armhf libfreetype-dev:armhf \
      libdbus-1-dev:armhf libudev-dev:armhf libglib2.0-dev:armhf \
      libx11-dev:armhf libxmu-dev:armhf libxcb-render0-dev:armhf libxcb-shape0-dev:armhf \
      libxcb-xfixes0-dev:armhf libxkbcommon-dev:armhf libxkbcommon-x11-dev:armhf \
      libwayland-dev:armhf libegl1-mesa-dev:armhf libgles2-mesa-dev:armhf zlib1g-dev:armhf \
    && rm -rf /var/lib/apt/lists/*

# mozjs configure wants unversioned llvm-objdump (and friends): Debian keeps them in /usr/lib/llvm-19/bin.
ENV RUSTUP_HOME=/opt/rustup PATH=/opt/cargo/bin:/usr/lib/llvm-19/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | CARGO_HOME=/opt/cargo sh -s -- -y --profile minimal --default-toolchain ${RUST_VERSION} \
    && rustup target add armv7-unknown-linux-gnueabihf

# mozjs configures with --with-arch=armv7-a, i.e. -march=armv7-a, which drops the FPU that Debian's gcc gets from
# its default armv7-a+fp, and hard-float then fails. The wrappers add the FPU Rust's armv7 gnueabihf target assumes.
RUN for t in gcc g++; do \
      printf '#!/bin/sh\nexec /usr/bin/arm-linux-gnueabihf-%s -mfpu=vfpv3-d16 "$@"\n' $t \
        > /usr/local/bin/armv7-$t && chmod +x /usr/local/bin/armv7-$t; done

# ARM EABI keeps type_info::operator== out of line; SpiderMonkey's hidden-visibility system-header wrappers then
# leave a hidden undefined reference that fails the final link. Inline equality (a name compare) avoids it.
# GCC predefines it as 0 on ARM: -U first, since a redefinition warning makes cc-rs drop flags like -fno-rtti.
ENV CARGO_HOME=/cargo \
    CC_armv7_unknown_linux_gnueabihf=armv7-gcc \
    CXX_armv7_unknown_linux_gnueabihf=armv7-g++ \
    AR_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-ar \
    CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc \
    CXXFLAGS_armv7_unknown_linux_gnueabihf="-U__GXX_TYPEINFO_EQUALITY_INLINE -D__GXX_TYPEINFO_EQUALITY_INLINE=1" \
    HOST_CC=gcc HOST_CXX=g++ LIBCLANG_PATH=/usr/lib/llvm-19/lib \
    MOZJS_FROM_SOURCE=1 PYTHON3=python3 PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG_LIBDIR_armv7_unknown_linux_gnueabihf=/usr/lib/arm-linux-gnueabihf/pkgconfig:/usr/share/pkgconfig \
    PKG_CONFIG_SYSROOT_DIR_armv7_unknown_linux_gnueabihf=/

WORKDIR /work
