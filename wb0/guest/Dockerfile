# SPDX-License-Identifier: Apache-2.0
# WB0 guest rootfs: Debian trixie armhf for qemu-system-arm -M virt. mkimage.sh exports it to an ext4 image.
FROM --platform=linux/arm/v7 debian:trixie

RUN apt-get update && apt-get install -y --no-install-recommends \
      linux-image-armmp-lpae initramfs-tools systemd-sysv systemd-resolved udev dbus \
      openssh-server ca-certificates procps iproute2 util-linux psmisc python3 curl file \
      fontconfig fonts-dejavu-core fonts-noto-core \
      libfontconfig1 libfreetype6 libdbus-1-3 libudev1 libglib2.0-0t64 \
      libx11-6 libxmu6 libxcb-render0 libxcb-shape0 libxcb-xfixes0 libxkbcommon0 libxkbcommon-x11-0 \
      libwayland-client0 libwayland-egl1 libegl1 libegl-mesa0 libgles2 libgl1-mesa-dri zlib1g \
    && rm -rf /var/lib/apt/lists/*

COPY guest_key.pub /root/.ssh/authorized_keys
RUN chmod 700 /root/.ssh && chmod 600 /root/.ssh/authorized_keys \
    && printf '[Match]\nName=en* eth*\n[Network]\nDHCP=yes\n' > /etc/systemd/network/20-dhcp.network \
    && systemctl enable systemd-networkd systemd-resolved ssh \
    && echo wb0 > /etc/hostname \
    && printf '/dev/vda / ext4 rw,relatime 0 1\n' > /etc/fstab \
    && mkdir -p /wb0 \
    && printf 'virtio_mmio\nvirtio_blk\nvirtio_net\nvirtio_rng\next4\n' >> /etc/initramfs-tools/modules \
    && update-initramfs -u -k all
