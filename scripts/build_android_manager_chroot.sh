#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT_DIR"

# Build manager.apk inside Ubuntu chroot to avoid Termux aapt2 segfaults.
#
# Requirements (device-side):
# - KernelSU / root available (needs `su` + `chroot` + mount).
# - Ubuntu rootfs exists at `$DSAPI_UBUNTU_CHROOT_ROOTFS` (default: ~/ubuntu-chroot/rootfs).
# - The repo is bind-mounted into the rootfs at `$DSAPI_CHROOT_REPO_DIR` (default: /work/DirectScreenAPI).
#
# Output: prints the same `android_manager_*` lines as build_android_manager.sh

OUT_DIR="${DSAPI_MANAGER_OUT_DIR:-artifacts/ksu_module/android_manager}"
ROOTFS="${DSAPI_UBUNTU_CHROOT_ROOTFS:-$HOME/ubuntu-chroot/rootfs}"
CHROOT_REPO_DIR="${DSAPI_CHROOT_REPO_DIR:-/work/DirectScreenAPI}"
HOST_REPO_DIR="${DSAPI_HOST_REPO_DIR:-$ROOT_DIR}"

if ! command -v su >/dev/null 2>&1; then
  echo "android_manager_chroot_error=su_missing"
  exit 2
fi
if ! command -v chroot >/dev/null 2>&1; then
  echo "android_manager_chroot_error=chroot_missing"
  exit 2
fi
if [ ! -d "$ROOTFS" ]; then
  echo "android_manager_chroot_error=rootfs_missing path=$ROOTFS"
  exit 2
fi

mount_if_needed() {
  mount_point="$1"
  mount_cmd="$2"
  if su -c "grep -F \" $mount_point \" /proc/mounts >/dev/null 2>&1"; then
    return 0
  fi
  su -c "$mount_cmd" >/dev/null 2>&1 || true
}

ensure_repo_mount() {
  # Ensure repo is visible inside chroot at `$CHROOT_REPO_DIR`.
  repo_mp="$ROOTFS$CHROOT_REPO_DIR"
  su -c "mkdir -p \"$repo_mp\"" >/dev/null 2>&1 || true
  if su -c "test -d \"$repo_mp/.git\""; then
    return 0
  fi
  if su -c "grep -F \" $repo_mp \" /proc/mounts >/dev/null 2>&1"; then
    return 0
  fi
  su -c "mount --bind \"$HOST_REPO_DIR\" \"$repo_mp\" >/dev/null 2>&1" || \
    su -c "mount -o bind \"$HOST_REPO_DIR\" \"$repo_mp\" >/dev/null 2>&1" || true
}

# Minimal mounts for a usable chroot.
su -c "mkdir -p \"$ROOTFS/proc\" \"$ROOTFS/sys\" \"$ROOTFS/dev\" \"$ROOTFS/dev/pts\"" >/dev/null 2>&1 || true
mount_if_needed "$ROOTFS/proc" "mount -t proc proc \"$ROOTFS/proc\""
mount_if_needed "$ROOTFS/sys" "mount -t sysfs sysfs \"$ROOTFS/sys\""
mount_if_needed "$ROOTFS/dev" "mount -t tmpfs tmpfs \"$ROOTFS/dev\" -o mode=755"
mount_if_needed "$ROOTFS/dev/pts" "mount -t devpts devpts \"$ROOTFS/dev/pts\" -o mode=600,ptmxmode=000"
ensure_repo_mount

chroot_env='HOME=/root TERM=xterm-256color PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'

# Use Debian android-sdk platform-23 as stable aapt2 include to avoid aapt2 crashes on some newer framework jars.
ANDROID_23_JAR="/usr/lib/android-sdk/platforms/android-23/android.jar"

# Use repo-local D8 wrapper to support Java 21 bytecode.
D8_WRAPPER="$CHROOT_REPO_DIR/.cache/d8/d8-wrapper.sh"

su -c "chroot \"$ROOTFS\" /usr/bin/env -i $chroot_env /bin/bash -lc \
  'set -eu; cd \"$CHROOT_REPO_DIR\"; \
    DSAPI_MANAGER_OUT_DIR=\"$OUT_DIR\" \
    DSAPI_MANAGER_VERSION_NAME=\"${DSAPI_MANAGER_VERSION_NAME:-0.1.0}\" \
    DSAPI_MANAGER_VERSION_CODE=\"${DSAPI_MANAGER_VERSION_CODE:-1}\" \
    DSAPI_MANAGER_JAVA_RELEASE=\"${DSAPI_MANAGER_JAVA_RELEASE:-21}\" \
    DSAPI_DEX_MODE=\"${DSAPI_DEX_MODE:-d8}\" \
    DSAPI_DEX_TOOL_BIN=\"$D8_WRAPPER\" \
    DSAPI_ANDROID_JAR=\"$ANDROID_23_JAR\" \
    DSAPI_AAPT2_ANDROID_JAR=\"$ANDROID_23_JAR\" \
    ./scripts/build_android_manager.sh' \
"

