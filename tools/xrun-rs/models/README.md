# Microsoft Effect Pack 模型提取与逆向（2026-10-17）

## 提取链路

1. `PerceptionCoreResources.dll`（31.5MB）的 PE 资源全是 RCDATA（type 10）
2. 五个数据块统一 **8 字节循环异或混淆，密钥 = `DROWSSAP`（"PASSWORD" 倒写）** — 频率分析破解（密钥字节 ⊕ 明文大量 0x00 → 密钥字符成为高频字节，全挤在 0x4F-0x57）
3. 解密后非标准 ONNX：**微软自定义 IR 信封**（protobuf）：
   - 字段 1 = producer "tf2onnx"
   - 字段 5 × 77 = 张量清单（TF 风格名 `(image:0)`，含 data_type/shape 属性，不含权重数据）
   - 字段 10 = 图（嵌套 2MB 大块 = 图结构+权重主体）
   - 字段 101 × 167 = 算子注册表（带英文文档注释，如 qlinear-matmul/broadcast_tile/channel-shuffle）

## 模型身份（张量名考证）

| 文件 | 大小 | 架构线索 | 判定 | Windows 效果名（引擎 DLL 字符串佐证） |
|---|---|---|---|---|
| id101.onnx | 2.3MB | `compress_v1` 逐点卷积骨干，输入 `image:0` | 人像分割 | BackgroundEffects.ActiveBokehEffect / BgBlur（含 SegmentationMaskRefiner） |
| id109.onnx | 3.5MB | `gen/TwoFrameArchai*` 双帧时序 | 时序网络 | EyeContact.Version / GazeDirector |
| id113.onnx | 14MB | `Detector/logitsP` + ResBlock×15 | 人脸/人体检测 | BaseFaceBodyDetectorFilter（自动取景） |
| id119.onnx | 10.5MB | `generator` GAN + Tanh 输出 | 风格化生成 | CreativeFilter.Supported |
| id132.onnx | 1MB | StatefulPartitionedCall 小网 | 待定 | 疑似人像光/辅助网 |

## 关键情报

- **模型全部经 Vitis AI（`vaip`）量化**（QuantizeLinear/qlinear 算子遍布）— MEP 在 AMD 平台就是走 **NPU 量化路线**，与本机 Ryzen AI NPU 同族
- 引擎辅助滤波器全在：OneEuroFilter、gaussianSmooth、ImageTemporalSmoother、ConsistencyFilter — 端到端管线 = 模型 + 经典时域平滑
- `WindowsEffectsCameraMediaSource.dll` = vcam 虚拟摄像头桥（WinRT AugmentedMediaSource），对 Linux 的等价物就是 v4l2loopback

## 可运行性结论

- **不可直接加载**：非标准 ONNX（自定义字段号 5/10/101），stock onnxruntime 拒收
- **理论可重建**：无加密无混淆（XOR 已破），字段 10 的图 schema 解码后可重发射为标准 ONNX；工作量 = 天级（167 算子映射 + 图/权重布局）
- **实用替代**：开源等效（MediaPipe 分割/检测、RVM）+ v4l2loopback 管线，性价比远高于重建

## 工具

- `/tmp/pe_res.py` PE 资源提取器（注意：资源子目录偏移是 .rsrc 节相对）
- `/tmp/onnx_info.py` 极简 ONNX protobuf 解析（对本 IR 仅顶层枚举有效）

---

# 第二阶段：推理调试记录（2026-10-17 下午）

## 重大进展

1. **格式身份确认**：提取的 5 个文件是 **XIR xmodel**（voe 序列化 = XIR GraphProto + vaip 扩展字段）。用 voe 1.8 自带工具直接读出：
   - `xir dump_txt id101.onnx` → 29882 行完整图文本（/tmp/id101_xir.txt）
   - `xir subgraph` → 子图树：**`device=DPU, fingerprint=0xa0002050110a5eb, DPU=AMD_AIE2P_4x4_Overlay_CFG0`** — **与本机 Strix NPU 完全匹配**
   - 骨干：mobilenet_v1 + compress_v1 量化压缩层，输入 480×480×3 XINT8，输出 (BiasAdd:0)_fix_ INT8
2. **RAI 1.8 Linux 全栈在 CachyOS 上跑通**（本机是 Ubuntu 24.04 目标，全部依赖手工适配）：
   - venv: `~/项目/mechrevo-star14/camera/mep-run/venv`（Python 3.12.15 via mise，numpy 必须 <2）
   - 运行环境: `bash rai-env.sh <cmd>`（含全部 LD_LIBRARY_PATH、XILINX_XRT、compat 兼容层）
   - `onnxruntime-vitisai 1.27` + `VitisAIExecutionProvider` 注册成功，EP 原生库全部加载
   - 兼容三件套：execstack 清除器（rai/clear_execstack.py，仓库内）、ncurses→ncursesw 符号链接（venv/compat）、配套 XRT 库（从 RAI_1.8_Linux_NPU_XRT.zip deb 里提取 libxrt_core.so.2.25.37，避免与 flm 装的旧版冲突）

## 卡点（老编译图 vs 新运行时的格式代差）

| 尝试 | 结果 |
|---|---|
| ORT 直接加载 xmodel | 标准解析器拒收（无 graph 字段） |
| EP cacheDir/cacheKey 平文件缓存 | EP 把文件归档为 `<key>.onnx` 后重编译会话模型，不执行旧格式 |
| `ep.context_file_path` | 是写侧（生成上下文），读侧只认 ORT tar 上下文 |
| waic 兼容配置（voe-4.0-linux tar 里的 vaip_config.json，含 microsoft 模式） | 走 `compile_onnx_model_vitisai_ep_v4` 老流程，但缓存必须是目录布局；平文件直接 fatal |
| pyflexmlrt（新运行时绑定） | 只认新 ml_txn/dd-ELF 事务格式（constinfo.json/wts32.bin/0/ml_txn.bin） |
| `show_dpu`（老 DPU 指令查看/执行） | `get_factory_methods()` 空 — 1.8 环境没挂老 DPU runner 插件 |
| 老 VART C API（libvitis-ai-runtime.so 导出 create_graph_runner_*_cif） | 存在但需要 C++ xir Graph 对象，无 pybind，ctypes 不可行 |

## 结论与三条前进路线

**格式代差**：模型是 2024-2025 年 vaip/voe 老流水线编译的 xmodel+PDI；2026 年 1.8 栈的执行器已换代到 ml_txn/dd-ELF。编译产物不可跨代直接执行（指令容器都换了）。

1. **旧版栈**（最可能成功）：RAI 1.3/1.4 Linux 的 EP 正是吃这种 xmodel 缓存的当代运行时 — 需再从 AMD 门户下载对应版本 tgz（同一 EULA 页面改 filename 参数）
2. **xcompiler 转换**：xir 工具能读（格式无加密），若 xcompiler 支持把老 xmodel 重编译为新格式（权重/属性都在），理论可行 — 工具 CLI 有 bug（help 崩溃），需试位置参数
3. **等效重建**：用开放权重（如 MediaPipe/MobileNet 分割）+ 已跑通的 1.8 栈原生编译 — 保底方案，架构参数（480×480、输入量化）已从逆向中拿全

## 资产清单

- `mep-run/venv` — 完整可用推理环境（对新模型立即可用；venv 不入库，按 INSTALL.md 自建）
- `rai/rai-env.sh` — 环境包装器（仓库通用化版，`RAI_VENV` 指向自建 venv）
- `rai/clear_execstack.py` — ELF 可执行栈标志清除器
- `/tmp/id101_xir.txt` — id101 完整图文本（重启后可重新生成）
- 事件/坑：numpy≥2 会让 ORT pybind 假死；PYTHONPATH 不能指文件；mise python 无 libpython 共享库需手动加路径

**补充验证**：`xcompiler -t CPU` 重编译被拒（"Cannot create children for non-leaf subgraph"）— 编译产物一经生成即锁定，无法跨代重编译。老版本直链探测（download.amd.com 各种路径 ryzen_ai-1.3/1.4.tgz）全部 404，旧栈只能走门户 EULA 再下载。
