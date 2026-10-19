# NOTICE — 仓库内厂商衍生素材清单

本仓库自有代码与文档按 MIT 授权。以下素材**衍生自第三方厂商发行物**，版权归原权利人所有，在此仅作硬件互操作研究与复现用途收录，**不适用 MIT**。此类二进制/权重文件**经 GitHub Release 资产（`xrun-assets.tar.gz`）分发而不进 git 树**。若权利人认为收录不当，请提 issue，将即时移除。

| 路径 | 来源 | 性质 |
|---|---|---|
| `integrations/audio/*.irs` `*.wav` 及原厂场景预设 | 机械革命 Windows 驱动包 Nahimic 组件的扬声器测量数据 | 测量数据衍生（卷积核/参数） |
| `tools/xrun-rs/models/*.onnx`（5 个） | Windows「Microsoft Effect Pack」`PerceptionCoreResources.dll` 资源段（8 字节循环 XOR 混淆，提取链路见 `models/README.md`） | 提取的模型权重（voe xmodel 格式） |
| `tools/xrun-rs/libs/libvitis-ai-runtime.so` 等运行库 | AMD Ryzen AI Software（voe wheel）发行包 | 二进制再分发 |
| `tools/xrun-rs/libs/libgraph-engine.so`、`libglog.so.0.6.0` | AMD RAI 配套发行物 | 二进制再分发 |
| `tools/xrun-rs/firmware/AMD_AIE2P_4x4_Overlay_CFG0.xclbin` | AMD voe-4.0 发行包 | NPU 覆盖固件 |
| `firmware/acpi/*` `firmware/ec/*` | 本机（P916F-STX，BIOS 1.15）ACPI 表反编译产物与 EC 素材 | 固件衍生 |

另：`docs/notes/` 中逆向分析引用了 UWACPIDriver.sys（Uniwill）等第三方驱动的静态分析结论，属合理研究引用，未收录其代码。
