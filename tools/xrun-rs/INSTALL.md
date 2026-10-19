# xrun-rs 安装与运行

Rust 直调 AMD XRT/Vitis-AI 运行库加载 NPU 模型的实验性推理器（绕过 Python，直接 dlopen `libvitis-ai-runtime.so` 的 C ABI）。在星耀14（Ryzen AI 9 365，Strix AIE2P 4x4 NPU）上以原厂 Windows 效果模型跑通过人像分割等图。

## 前提

- Rust 工具链
- **AMD XRT**（`/opt/xilinx/xrt/lib`）——随 [Ryzen AI Software](https://ryzenai.docs.amd.com) 安装；CachyOS/Arch 上无打包，需从 RAI 发行包提取（本机实测细节见 `models/README.md` 第二阶段与仓库 docs）
- NPU 为 Strix Point `AMD_AIE2P_4x4_Overlay_CFG0`（Ryzen AI 9 365/375 同族；其他代 NPU 需换 firmware/）

## 运行资产（GitHub Release 分发，不入 git 树）

模型权重、运行库（21M 的 `libvitis-ai-runtime.so` 等）、NPU 固件 xclbin 属厂商衍生素材，打包放在仓库 [Releases](../../releases) 的 `xrun-assets.tar.gz`（约 24M）。**构建运行前先解包**（在仓库根执行，自动恢复 `models/ libs/ firmware/` 布局）：

```bash
# 方式一：gh（推荐）
gh release download -R dwdcth/mechrevo-xingyao14-linux-tweaks -p 'xrun-assets.tar.gz' -O /tmp/xrun-assets.tar.gz
# 方式二：curl
curl -LO https://github.com/dwdcth/mechrevo-xingyao14-linux-tweaks/releases/latest/download/xrun-assets.tar.gz

tar -C tools/xrun-rs -xzf /tmp/xrun-assets.tar.gz
```

- `models/`：从 Windows「Microsoft Effect Pack」提取的 5 个模型（voe xmodel 格式；提取链路与格式逆向全记录在 [models/README.md](models/README.md)）
- `libs/`：运行库（`libvitis-ai-runtime.so` 真身 + graph-engine + glog，软链已相对化）
- `firmware/AMD_AIE2P_4x4_Overlay_CFG0.xclbin`：NPU 覆盖固件（7.6M）

## 构建与运行

```bash
cargo build --release
# 单次推理（自动 sudo prlimit memlock + 设定 XLNX_VART_FIRMWARE）
bash run.sh models/id101.onnx /tmp/in101.bin /tmp/out101.bin
```

`run.sh` 做了三件事：`prlimit --memlock=8G`（NPU 驱动锁页需求）、设 `XLNX_VART_FIRMWARE` 指向仓库内 xclbin、`LD_LIBRARY_PATH=/opt/xilinx/xrt/lib:./libs`。环境变量 `XLNX_VART_FIRMWARE` 可覆盖固件路径。

`run-vcam.sh` 为虚拟摄像头长跑模式（配合 v4l2loopback）。

## 验证

输入张量与输出的 MD5 基准表在 [README.md](README.md)（id101/109/113/119/132 五个模型），`cmp` 逐位比对通过即链路完好。

## 已知坑（CachyOS/Arch 特供）

- RAI 栈目标 Ubuntu：部分 .so 带可执行栈标志需清除；ncurses→ncursesw 符号链接；XRT 库版本要配套（避免与系统旧 libxrt_core 冲突）
- numpy 必须 <2（Python 侧推理时）；本仓库 Rust 路径不涉及

## 模型来源与再分发提示

models/ 权重提取自微软驱动资源段（8 字节循环 XOR 混淆，密钥见 models/README.md），经 GitHub Release 分发，仅供互操作研究，见根目录 [NOTICE.md](../../NOTICE.md)。
