# xrun-rs 安装与运行

Rust 直调 AMD XRT/Vitis-AI 运行库加载 NPU 模型的实验性推理器（绕过 Python，直接 dlopen `libvitis-ai-runtime.so` 的 C ABI）。在星耀14（Ryzen AI 9 365，Strix AIE2P 4x4 NPU）上以原厂 Windows 效果模型跑通过人像分割等图。

## 前提

- Rust 工具链
- **AMD XRT**（版本与安装见下节）
- 内核带 `amdxdna` NPU 驱动（主线已合并，CachyOS/Arch 7.x 内核自带；`ls /dev/accel/accel0` 或 `lsmod | grep amdxdna` 确认）
- NPU 为 Strix Point `AMD_AIE2P_4x4_Overlay_CFG0`（Ryzen AI 9 365/375、HX 370 同族；其他代 NPU 需换 firmware/）

## 安装 XRT（版本与下载）

**验证过的版本：XRT `202620.2.25.37`（RAI 1.8 配套，关键库 `libxrt_core.so.2.25.37`）+ amdxdna 插件 `2.25.260102.56`**。仓库资产里的 `libvitis-ai-runtime.so` 就是对这套版本链接的，换其他 XRT 版本未验证。

下载（EULA 表单，选 1.8 → Linux）：

- 门户：https://www.amd.com/en/products/software/ryzen-ai-software.html
- 安装文档：https://ryzenai.docs.amd.com/en/latest/inst.html
- 取 **`RAI_1.8_Linux_NPU_XRT.zip`**（86M，含四个 deb：base / base-dev / npu / plugin）
- xrun-rs 只需要 XRT；若还要 Python 全栈（onnxruntime-vitisai 等），另下 `ryzen_ai-1.8.0.tgz`（7.3G），Arch 系适配坑见 [models/README.md](models/README.md) 第二阶段

Ubuntu 24.04 直接 `sudo dpkg -i` 三个运行时 deb（base、npu、plugin）。**Arch/CachyOS 不能装 deb**，解包提库：

```bash
unzip RAI_1.8_Linux_NPU_XRT.zip && cd RAI_1.8_Linux_NPU_XRT
mkdir rootfs
for d in xrt_202620.2.25.37_24.04-amd64-base.deb \
         xrt_202620.2.25.37_24.04-amd64-npu.deb \
         xrt_plugin.*.deb; do
    ar p "$d" data.tar.gz | bsdtar -xf - -C rootfs
done
sudo cp -a rootfs/opt/xilinx/xrt /opt/xilinx/    # 库全部位于 opt/xilinx/xrt/lib
ls /opt/xilinx/xrt/lib | grep 'libxrt_core.so.2'  # 应见 libxrt_core.so.2.25.37
```

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
