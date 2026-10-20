# rai/ — RAI Linux 栈的 Arch/CachyOS 适配件

从 AMD 门户（EULA 页面）下载 Ryzen AI 1.8 Linux tgz 自建 venv 时，Ubuntu 目标的栈在 Arch 系上需要两个小工具，配合 [INSTALL.md](../INSTALL.md) 的三坑说明使用。

## 文件

- `rai-env.sh` — 环境包装器：一次设全 `RYZEN_AI_INSTALLATION_PATH` / `LD_LIBRARY_PATH`（compat、peano、flexml、voe、deployment、libpython）/ `XILINX_XRT` 等。venv 位置用 `RAI_VENV` 传入，其余自动探测
- `clear_execstack.py` — ELF `PT_GNU_STACK` 的 `PF_X` 位清除器：Ubuntu 构建的 `.so` 带可执行栈标志会被 Arch 内核直接拒载（`cannot load shared object file: Operation not permitted` 家族）

## 用法

```bash
# 包装器跑任意命令（验证栈完整性）
RAI_VENV=~/rai/venv rai-env.sh ~/rai/venv/bin/python \
    -c "import onnxruntime as ort; print(ort.get_available_providers())"
# 应包含 VitisAIExecutionProvider

# 新拷的 .so 被内核拒载时清标志
python3 clear_execstack.py <file.so>
```

## 三坑速查（详见 INSTALL.md / models/README.md）

1. **execstack**：Ubuntu `.so` 可执行栈标志 → 本目录 `clear_execstack.py` 清
2. **libncurses.so.6**：Arch 只有 ncursesw → venv 的 `compat/` 里建 `libncurses.so.6 → libncursesw.so.6` 符号链接
3. **libxrt_core.so.2**：系统旧版缺符号 → 从 `RAI_1.8_Linux_NPU_XRT.zip`（download.amd.com）内 deb 提取 2.25.37 版放 `compat/`

另：venv 必须 **numpy<2**（否则 ORT pybind 报 import 假死）；mise 装的 Python 无共享库，`rai-env.sh` 自动探测 `LIBDIR`，探测不到时用 `RAI_PY_LIB` 指定。
