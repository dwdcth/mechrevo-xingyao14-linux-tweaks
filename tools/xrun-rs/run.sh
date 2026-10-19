#!/bin/bash
# run.sh — xrun-rs 一键运行（RAI 1.8 栈：voe 合并运行库 + 系统 XRT，无垫片/无外部引擎）
# 用法: bash run.sh <model.onnx> <input.bin> <output.bin>
# 依赖: sudo（prlimit memlock）、/opt/xilinx/xrt、Release 资产（见 INSTALL.md）
set -e
XR=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
# voe 运行库已收入本仓库 libs/
XCL=${XLNX_VART_FIRMWARE:-$XR/firmware/AMD_AIE2P_4x4_Overlay_CFG0.xclbin}
[ -f "$XCL" ] || { echo "缺运行资产：先下载 Release 的 xrun-assets.tar.gz 并在仓库根解包（见 INSTALL.md）" >&2; exit 1; }
exec sudo prlimit --memlock=8589934592 -- env \
  XLNX_VART_FIRMWARE="$XCL" \
  XLNX_VART_SKIP_FP_CHK=TRUE \
  LD_LIBRARY_PATH=/opt/xilinx/xrt/lib:$XR/libs \
  "$XR/target/release/xrun-rs" "$1" "$2" "$3"
