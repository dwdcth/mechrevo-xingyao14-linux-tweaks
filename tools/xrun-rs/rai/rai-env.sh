#!/bin/bash
# RAI Linux 1.8 推理环境包装器（Arch/CachyOS 适配）
# 用法: rai-env.sh <任意命令>
#   例: RAI_VENV=~/rai/venv rai-env.sh ~/rai/venv/bin/python train.py
# 可用环境变量覆盖:
#   RAI_VENV  自建 venv 根目录（默认: 本脚本旁的 venv/）
#   RAI_XRT   XRT 安装目录（默认: /opt/xilinx/xrt）
#   RAI_PY_LIB 提供 libpythonX.Y.so.1.0 的目录（默认从 venv 的 Python 自动探测；
#               mise/源码编译的 Python 无共享库进 venv，需要时手动指定）
V="${RAI_VENV:-$(dirname "$(readlink -f "$0")")/venv}"
if [[ ! -x "$V/bin/python" ]]; then
    echo "Error: venv not found at '$V' (set RAI_VENV to your RAI venv)" >&2
    exit 1
fi
PYV=$("$V/bin/python" -c 'import sys; print(f"python{sys.version_info.major}.{sys.version_info.minor}")')
SP=$V/lib/$PYV/site-packages
PY_LIB="${RAI_PY_LIB:-$("$V/bin/python" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))' 2>/dev/null)}"
export RYZEN_AI_INSTALLATION_PATH=$V
export LD_LIBRARY_PATH=$PY_LIB:$V/compat:$SP/lnx64.o/tools/peano/lib:$SP/flexml/flexml_extras/lib:$SP/onnxruntime/capi:$SP/voe/lib:$V/deployment/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
export XILINX_XRT="${RAI_XRT:-/opt/xilinx/xrt}"
export XILINX_VITIS=$SP
export XILINX_VITIS_AIETOOLS=$SP
exec "$@"
