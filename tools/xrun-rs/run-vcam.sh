#!/bin/bash
# run-vcam.sh — xrun-rs vcam（Windows 背景虚化管线）一键运行（RAI 1.8 栈）
# 用法: bash run-vcam.sh [-- vcam 参数...]
#   例: bash run-vcam.sh                       # /dev/video0 → /dev/video9 实时虚化
#       bash run-vcam.sh -- --bg-mode portrait --bg-strength 0.7
#       bash run-vcam.sh -- --frames 5 --out png:/tmp/acc --dump-dir /tmp/dump
set -e
[ "$1" = "--" ] && shift
XR=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
XCL=${XLNX_VART_FIRMWARE:-$XR/firmware/AMD_AIE2P_4x4_Overlay_CFG0.xclbin}
[ -f "$XCL" ] || { echo "缺运行资产：先下载 Release 的 xrun-assets.tar.gz 并在仓库根解包（见 INSTALL.md）" >&2; exit 1; }
exec sudo prlimit --memlock=8589934592 -- env \
  XLNX_VART_FIRMWARE="$XCL" \
  XLNX_VART_SKIP_FP_CHK=TRUE \
  LD_LIBRARY_PATH=/opt/xilinx/xrt/lib:$XR/libs \
  "$XR/target/release/xrun-rs" vcam "$@"
