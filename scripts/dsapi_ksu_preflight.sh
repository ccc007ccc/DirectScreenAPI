#!/bin/sh
set -eu

pass() { echo "[PASS] $1"; }
warn() { echo "[WARN] $1"; }
fail() { echo "[FAIL] $1"; }

HAS_FAIL=0

if command -v su >/dev/null 2>&1; then
  if su -c id >/dev/null 2>&1; then
    pass "root 可用（su）"
  else
    fail "su 存在但不可用"
    HAS_FAIL=1
  fi
else
  fail "未找到 su"
  HAS_FAIL=1
fi

SDK="$(getprop ro.build.version.sdk 2>/dev/null || echo 0)"
if [ "$SDK" -ge 29 ] 2>/dev/null; then
  pass "Android SDK=$SDK"
else
  warn "Android SDK=$SDK，低版本未覆盖测试"
fi

if [ -d /data/adb/modules ]; then
  pass "/data/adb/modules 存在"
else
  fail "未找到 /data/adb/modules，疑似非 KSU/Magisk 环境"
  HAS_FAIL=1
fi

if [ -d /data/adb/ksu ]; then
  pass "检测到 /data/adb/ksu"
else
  warn "未检测到 /data/adb/ksu，可能不是 KernelSU（也可能是兼容布局）"
fi

SELINUX="$(getenforce 2>/dev/null || echo unknown)"
if [ "$SELINUX" = "Enforcing" ]; then
  pass "SELinux=Enforcing"
else
  warn "SELinux=$SELINUX（与预期不一致）"
fi

if command -v setenforce >/dev/null 2>&1; then
  pass "setenforce 存在"
else
  warn "setenforce 不存在，排障手段受限"
fi

if [ -x target/release/dsapid ] && [ -x target/release/dsapictl ]; then
  pass "核心二进制已存在"
else
  warn "核心二进制不存在，将在 pack 时自动构建"
fi

if [ "$HAS_FAIL" = "1" ]; then
  echo "ksu_preflight_status=failed"
  exit 2
fi

echo "ksu_preflight_status=ok"
