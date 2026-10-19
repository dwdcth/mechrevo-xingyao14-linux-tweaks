# Windows MEP (Microsoft Effect Pack) 效果参数体系逆向

> 2026-10-17 · 静态分析 PerceptionCore.dll 2.4.20（r2 + r2ghidra，只读）
> 样本：`microsofteffectpack.inf_amd64_b53451a00dda6a36/`（引擎）+ `mep_camera_component.inf_amd64_8ef4a516cc7f2af6/`（vcam 桥）
> 结论服务于 xrun-rs：给出可暴露成 CLI/配置的参数、默认值、以及 Windows 侧"手感"的后处理常量。

## 0. 组件链路（先讲清楚架构）

```
App (WinRT Windows.Camera.Effects / MF)
 │  IKsControl::KsProperty
 ▼
WindowsEffectsCameraMediaSource.dll   (vcam 虚拟摄像头, 命名空间 mep::vcam, C++/WinRT)
 │  导出 DllGetClassObject → EffectsCameraMediaSourceActivate
 │  加载 PerceptionCore.dll, 调 4 个导出:
 │    PerceptionCoreCreate / PerceptionCoreDestroy(WithStatus)
 │    PerceptionCoreGetAllScenarios
 │  属性经 PERCEPTIONCOREPROPKEY_* (PROPVARIANT/GUID 键) 下发
 ▼
PerceptionCore.dll  (引擎, Microsoft.ASG.Perception, RTTI 全保留)
 │  VitisLib → vitis-ai-runtime.dll → NPU (DPU, AMD_AIE2P)
 ▼
PerceptionCoreResources.dll  (7 个 PE 资源: 5×RCDATA 模型 + VERSIONINFO + manifest)
```

- 引擎版本串：`PerceptionCore-v2.4.20`；遥测 provider `Microsoft.ASG.Perception`（ASG = Applied Sciences Group，Surface 摄像头团队）。
- vcam 构建路径：`D:\a\_work\1\s\Camera\EffectsCamera\EffectsCameraMediaSource\x64\Strix\` —— 明确按 **Strix** 构建（与本机 Ryzen AI 同代）。
- 引擎配置 = **nlohmann::json**（运行时传入的 JSON blob，非注册表）。所有 `X.Y` 键都是 JSON 键，缺省时走代码内默认值（下表"默认"列就是硬编码值）。

## 1. 效果 → 模型 → 场景位 总表

### 1.1 场景位（scenario bitmask，`SetEffectScenarios`/`requested scenarios %llx`）

| 位 | 含义 | 证据 |
|---|---|---|
| 0x4 | EyeContact/双帧分割消费（EGC 的最可能映射位；vcam bit19→0x824 含此位） | 在 0x14177 能力掩码内 |
| 0x10 | PreBake（预烘焙光照） | fcn.180010b40；StageLight 的前置依赖 |
| 0x20 | 分割依赖管线（SEG 家族） | 同上 |
| 0x40 | **背景虚化 BLUR** | 同上 + fcn.180015170 |
| 0x100 | FacePoseLandmarksEstimator 管线（面部元数据/眼神接触前置） | fcn.18001a1c0 构造 `std::_Ref_count_class_FacePoseLandmarksEstimator` |
| 0x4000 | 背景效果变体（SHALLOWFOCUS 浅景深） | fcn.180015170（384×216 mask 分支） |
| 0x10000 | 创意管线（pipeline a，CreativeFilter） | fcn.180010b40 |
| 0x20000 | **StageLight（舞台光）** | vcam: bit17 置位时自动补 0x10；状态 bit20 → 0x20003c |
| 0x100000 | （推断）EyeContact/高阶效果总开关位——注意不在 0x14177 能力掩码内，vcam 状态 bit5 → 0x10003c 的请求会被部分拒掉 | vcam ProcessSample 位映射 |
| 0x84060 | "背景效果家族"总掩码（bits 5,6,14,19） | fcn.180014d40 入口检查 |
| 0x200000 | 高分辨率模式（抑制 bokeh） | fcn.180015170 |

vcam 状态字 → 引擎场景位的完整加法映射（ProcessSample，0x18006b8xx）：state bit1→+0x30、bit2→+0x301c、bit8→+0x3030、bit10→+0x2c、bit5→+0x10003c、bit20→+0x20003c、bit16→+0x24、bit19→+0x824、bit60/61/62（大机位掩码）→+0x30/+0x34/+0x51f4。

引擎能力掩码硬常量：`PerceptionCoreGetAllScenarios()` 直接 `return 0x14177`（0x180005480），即对外宣称支持 bits {0,1,2,4,5,6,8,14,16} —— 含 BLUR(0x40)/SHALLOW(0x4000)/创意(0x10000)/FacePose(0x100)/EGC 候选(0x4)，不含 0x20000/0x80000（本 SKU 无 StageLight 管线；EGC 查询若命中不支持的位会走 "PerceptionCore does NOT support KSPROPERTY_CAMERACONTROL_EXTENDED_EYEGAZECORRECTION" 回退路径）。

内部管线名（引擎日志字符串）：`Pipeline a`（创意）、`Pipeline ab1b2`、`Pipeline c`/`Model C4`、`model d`、`model e`、`Pipeline ActiveDirector`；
阶段名：`AD_GENCROP` `SEG_ROI` `BG_PRE` `SEG_MR` `SL_COMP` `synch_FDFT`（人脸检测+跟踪）`synch_SEG` `synch_BG_PRE` `synch_BG` `synch_DE` `synch_AD_UP` `synch_SL_COMP` `synch_SEG_MR`。

### 1.2 模型槽位（AlgoFactoryComputeCore 虚方法 → 资源 ID → 消费类）

| 资源 ID | 大小 | 消费类（RTTI 证实） | 效果 | 说明 |
|---|---|---|---|---|
| **101** | 2.29MB | `CCFaceBodyDetector`（经 BaseFaceBodyDetectorFilter） | 人脸/人体检测（喂 AD 取景、FaceTracker 初筛） | **修正**：前判"人像分割"不准；480×480 输入的 compress_v1 检测骨干 |
| **109** | 3.50MB | `CCForegroundSegmentation` | **双帧时序分割**（背景虚化/眼神接触共用） | 输入 `normed_rgbm:0`（RGB+上帧mask），输出 `PostProcess/stage0/x0.5/seg:0` 与 `x1/seg:0` 两档分辨率 |
| **113** | 14.0MB | `FacePoseLandmarksEstimator` | 人脸关键点+姿态（FACE_POSE_LANDMARKS，面部元数据/眼神接触前置） | **修正**：前判"人脸检测"不准 |
| **119** | 10.5MB | `GazeDirector` | **EyeContact 眼神接触**（GAN 重定向生成器） | 仅当 `EyeContact.Version == 2` 才加载（默认 2 → 默认启用）；隐向量输入 64×128×8 |
| **132** | 1.03MB | `SuperRes`（FaceSuperRes） | 人脸超分（Superres.Model 选 1X/2X/4X） | 默认 `NONE` 不启用；默认张量尺寸 384×384×3（常量 0x1801ecf20） |
| 103 | — | CCFaceLandmarksDetector（FACE_LANDMARKS_28 粗跟踪） | 可选槽位 | **本资源包未包含**（其他 SKU 用） |
| 105 | — | CCFaceLandmarksDetector（FACE_LANDMARKS_320 精细化，"model B2"） | 可选槽位 | 未包含 |
| 140 | — | CCFaceLandmarksDetector（EYE_LANDMARKS/EYE_LANDMARKS_BATCHED/96，"model B1"） | 可选槽位 | 未包含 |

> 资源清单是硬事实：PerceptionCoreResources.dll 的资源树**只有 5 个 RCDATA**（type10：101/109/113/119/132）+ VERSIONINFO(16) + manifest(24)。Mechrevo 这套包里 FaceTracker 走 113（FACE_POSE_LANDMARKS 复用缓存），103/105/140 找不到时静默降级。

### 1.3 id119 创意滤镜子类型（GAN 的驱动方式）

- **id119 不是创意滤镜 —— 是 EyeContact 的 GazeDirector GAN**（眼线重定向生成器）。创意滤镜（CreativeFilter）在 vcam 层由 `KSPROPERTY_CAMERACONTROL_WINDOWSSTUDIO_CREATIVEFILTER` 控制，传给引擎的只是一个 int（`PERCEPTIONCOREPROPKEY_CREATIVEFILTER_ACTIVE`，值 = 风格编号）。
- 风格枚举（vcam 字符串 + 官方 Windows-Camera 样例 `KsHelper.cs` 双重证实）：
  - `KSCAMERA_WINDOWSSTUDIO_CREATIVEFILTER_OFF = 0`
  - `KSCAMERA_WINDOWSSTUDIO_CREATIVEFILTER_ILLUSTRATED = 1`
  - `KSCAMERA_WINDOWSSTUDIO_CREATIVEFILTER_ANIMATED = 2`
  - `KSCAMERA_WINDOWSSTUDIO_CREATIVEFILTER_WATERCOLOR = 4`（位掩码可组合）
- vcam 侧另有 `PERCEPTIONCOREPROPKEY_CREATIVEFILTER_SUPPORTED`（回读支持的风格集合，失败时"assuming all filters are supported"）。
- 创意滤镜的实际生成网络在哪？引擎内 pipeline `a`/`ab1b2` + `model g`/`model d`/`model e` 字符串提示 GAN 的 g/d（生成/判别）——但**资源包里没有对应 RCDATA**。推断：Mechrevo SKU 的创意滤镜 = 半成品（`CreativeFilter.Supported` 默认空 → vcam 直接报 "No CREATIVEFILTER supported"），Windows Studio Effects 的创意滤镜模型不在本包。

### 1.4 id109（双帧分割）驱动机制

`CCForegroundSegmentation::process`（method.virtual_16，0x180037594 起）+ 输入装配 `fcn.180037ea0`：

1. `normed_rgbm` 4 通道 = **当前帧 RGB（3ch，来自 +0x28 成员）+ 上一帧分割 mask（+0x48 成员）**；
2. mask 缓冲首帧用 `memset 0`（黑=无前景先验），之后每帧推理输出回写 → 时序反馈环（这就是"双帧时序"的本质：显式 mask 反馈，而非两幅 RGB）；
3. 配置对象 +0xc0 的 mode 字节（来自 `Segmentation.WFOV` 配置）非 0 时走**宽 FOV 双分支布局**（WFOV=1：RGB 两份 + mask，对应 128×224×4 张量布局）；
4. 输出两档：`x0.5/seg`（半分辨率粗图）与 `x1/seg`（全分辨率），BgBlur 用高档，ActiveDirector 用低档。

## 2. 用户可调参数表（引擎 JSON 键 / KS 属性）

### 2.1 引擎 JSON 配置键（nlohmann::json，注册函数 fcn.180025590 + 各读取点）

| 键 | 类型 | 默认 | 范围/枚举 | 消费者 |
|---|---|---|---|---|
| `FaceDetection.FaceThreshold` | float | 读取点缺省 0（范围下限） | FloatRangeProp: **0.0 ~ 1.0**（注册处 +0x28/+0x2c 两 float） | CCFaceBodyDetector 检测阈值 |
| `FaceDetection.Filter` | enum/int | — | — | 检测过滤器 |
| `FaceTracker.CoarseModel` | string | `FACE_POSE_LANDMARKS` | FACE_LANDMARKS_28(0) / FACE_POSE_LANDMARKS(1) / EYE_LANDMARKS(2) / EYE_LANDMARKS_BATCHED(3) / FACE_LANDMARKS_96(4) / FACE_LANDMARKS_320(5) | 粗跟踪模型选择（本包=113） |
| `FaceTracker.RefinerModel` | string | `NONE`→FACE_LANDMARKS_320 | 同上 | 精细化模型（105，未装配） |
| `FaceTracker.NumFacesToTrack` | int | **1** | ≥1 | BaseFaceLandmarksDetector |
| `Segmentation.RefinerMode` | string | `NONE` | `NONE` / `FEATHERING` | mask 后处理（羽化） |
| `Segmentation.WFOV` | int/bool | 0 | 0/1 | id109 双分支布局开关 |
| `ActiveDirector.RefinerMode` | string | `NONE` | `NONE`/`AUTO`/`FACE`/`FULL_FRAME` | 自动取景裁切策略 |
| `EyeContact.Version` | int | **2** | 2 = GAN 路线（加载 id119） | GazeDirector 门控 |
| `Superres.Model` | string | `NONE` | `NONE`/`SUPERRES_1X`/`SUPERRES_2X`/`SUPERRES_4X` | id132 |
| `ParallelFor.NumThreads` | int | — | — | 线程池 |
| `WorkQueueID` | int | -1（无 MF 队列→内建线程池） | — | 调度 |
| `BackgroundEffects.ActiveBokehEffect` | int | -1（缺省） | 0/1；=1 且 0x200000 模式时强制 0 | 音频驱动虚化（AudioBlur metadata） |
| `Camera.OptIn.Type` / `Camera.ACPIPLDPanel.Enum` | int | vcam 下发 | 面板位置 | 设备适配 |
| `CreativeFilter.Supported` | — | 空 | — | vcam 回读 |

### 2.2 Windows 属性面（vcam IKsControl::KsProperty 拦截表）

**属性集 A：`KSPROPERTYSETID_ExtendedCameraControl` {1CB79112-C0D2-4213-9CA6-CD4FDB927972}**（标准 ksmedia.h，已在二进制 0xdbd90 验证）

| KSPROPERTY ID | 处理器 | 能力位（KSCAMERA_EXTENDEDPROP） |
|---|---|---|
| 0 | WindowsCameraEffect API 分发 | — |
| 40 `EXTENDED_EYEGAZECORRECTION` | EyeGazeCorrectionPropertyHandler | OFF=0 / ON=1 / **STARE=2**（凝视加强档） |
| 41 `EXTENDED_BACKGROUNDSEGMENTATION` | BackgroundSegmentationPropertyHandler | OFF=0 / BLUR=1 / MASK=2 / **SHALLOWFOCUS=4**；ConfigCaps 含 MaskResolution/MaxFrameRate/SubType |
| 43 `EXTENDED_DIGITALWINDOW` (+45/46 ConfigCaps/FOV2) | DigitalWindowPropertyHandler | 自动取景窗口 |
| 44 `EXTENDED_FRAMERATE_THROTTLE` | FramerateThrottle | OFF=0/ON=1（降帧省电） |
| 46 `EXTENDED_FIELDOFVIEW2` | — | 视场角 |
| 14/18/22/34 等 | Metadata/ROI/Profile 转发 | ForwardUnhandledKSProperties 透传物理相机 |

**属性集 B：`KSPROPERTYSETID_WindowsCameraEffect` {1666D655-21A6-4982-9728-52C39E869F90}**（Windows Studio Effects，已在 0xde690 验证；ID 来自官方 WindowsStudio 样例 KsHelper.cs）

| ID | API | 备注 |
|---|---|---|
| 0 | WINDOWSSTUDIO_SUPPORTED | 能力查询 |
| 1 | WINDOWSSTUDIO_STAGELIGHT | OFF/ON → 场景位 0x20000（拉起 PreBake 0x10） |
| 2 | WINDOWSSTUDIO_CREATIVEFILTER | OFF/ILLUSTRATED(1)/ANIMATED(2)/WATERCOLOR(4) |
| 3 | WINDOWSSTUDIO_SETNOTIFICATION | 变更通知 |
| 4 | WINDOWSSTUDIO_PERFORMANCEMITIGATION | 性能回退（isPerfMitigated） |
| 5 | WINDOWSSTUDIO_FACEMETADATA | OFF/FACETRACKING(1)/FACELANDMARKS(2)/FACEPOSE(4)，元数据 GUID：FACETRACKING {F008811B-70C6-4A4F-9424-D576AC159905}、FACELANDMARKS {1239D90E-6920-49A9-B72E-5F6D77944675}、FACEPOSE {397757AA-B630-43CE-AD47-AB3AC5FE4728}；MAX_NUM_LANDMARKS=70 |
| 6 | WINDOWSSTUDIO_SENSORCENTERCROP | — |
| 7 | WINDOWSSTUDIO_AUTOMATICFRAMINGKIND | WINDOW=1 / **CINEMATIC=2**（电影感取景） |
| 8 | WINDOWSSTUDIO_UPDATED_VIEWPORT | — |

**注册表 opt-in**：`IsOptedIntoMEPViaRegKey` → `CM_Open_Device_Interface_KeyW(相机接口, KEY_READ)` + `RegGetValueW(..., "FSMEnableMsEffects")` DWORD（相机设备接口键下，非全局键）。

## 3. 后处理滤波器参数（Windows"手感"的来源）

### 3.1 全局浮点常量池（.rdata 0x1801ecb2c-0x1801ece44，部分）

`0.001, 0.0012, 0.00138889(=1/720), 0.00392157(1/255), 0.015, 0.02, 0.0227, 0.035, 0.0357143, 0.05, 0.06, 0.0625, 0.07, 0.0813, 0.09, 0.1, 0.114, 0.12, 0.125, 0.15, 0.2, 0.225, 0.24, 0.25, 0.3, 0.32, 0.45, 0.5, 0.55, 0.6, 0.65, 0.693147(ln2), 0.7, 0.75, 0.8, 0.81, 0.85, 0.88, 0.9, 0.94, 0.99, 1.0, 1.05, 1.1, 1.2, 1.25, 1.5, 1.875, 2.0, 2.5, 3.0, 3.14159, 4, 5, 6, 6.28319, 7, 8, …, 22.4, 25.7253, 30, 32.6566, 33.3333, 44, 50, 57.2958(度→弧度), 85, 89.99, 90, 96, 98, 120, 124.225, 126.943, 127.5, 128, 180, 224, 255, 256, 268.8, 295, 300, 448, 1280, 4096` 及负数区 `−0.065, −0.5, −1, −1.5, −1.875, −2, −10, −85, −89.99`。
其中 ±85 / ±89.99 是**视线俯仰/偏航角钳位（度）**，124.225/126.943 是色彩空间界限（Lab 类变换）。

### 3.2 类 → 参数 → 值

**BgEffects::SegmentationMaskRefiner**（构造 0x180014d40 内联，对象 0x360B）
- 时域混合系数 **0.3**（`0x3e99999a`，+0x18）—— mask EMA：`out = (1−α)·prev + α·new`，α=0.3
- 时域重置阈值 **0.02**（`0x3ca3d70a`，+0xd0）—— 帧间 RMSE 超过 2% 即重置历史（场景切换）
- mask 输出定标 **×255.0**（0x1801ecdd8），1.0 = 单位权重（0x1801ecc24）
- ROI 钳位常量 540×960（0x21c/0x3c0）

**BgEffects::BackgroundEffectsLib**（构造于 fcn.180010b40，对象 0x68B）
- 两个 **0.3f** 时域平滑（+0x14/+0x28）
- 羽化半径 25（+0x10），迭代 7
- BLUR 模式（场景位 0x40）：内部 mask **192×108**，bokeh 核 **4.0f**（fcn.180015170）
- SHALLOWFOCUS 模式（0x4000）：mask **384×216**，核 **3.0f**
- 对焦区 = 人脸框（4 float，随相机 90/180/270° 旋转重映射）

**ImageTemporalSmoother\<float\>**（fcn.1800396f0，被 SegmentationMaskRefiner 内嵌）
- 逐像素 EMA α=0.3 + 帧间 RMS 触发全量重置（阈值 0.02，同上）—— Windows 背景虚化"不闪烁"的核心。

**FaceTracker / BaseFaceLandmarksDetector**（构造参数块，virtual_40 + fcn.18002bf60）
- 通用块：`0.45, 0(int), 30, 1.0, 16, NumFacesToTrack(默认1), 0.025, 0.07, −0.15, 50, 0.99(跟踪置信度), 257, 2, 30.0(最小脸尺寸px), 0.65, 0.55, 1.0, 1, −1`
- 按 CoarseModel 覆盖：
  - FACE_LANDMARKS_28: 检测阈 **0.1**
  - FACE_POSE_LANDMARKS(本包): **0.125 / 0.9 / 8 / 0.25**
  - FACE_LANDMARKS_96/320: **0.2 / 0.5 / −0.005 / 0.1**

**GazeDirector（id119，EyeContact v2）**（构造参数块，virtual_56 → fcn.18013c4f0，对象 0x820B）
- `1, 137.555f, 105.863f, 90.294f`（人脸几何先验：瞳距/眼宽像素级常量，512 空间）
- 容差/步长：`0.04, 0.05, 0.025, 0.033`（迭代收敛参数）
- `2.0, 50, 256, 10, 0.5, 128, 64, 8, 10, 2, 65536, 81920`（尺寸/上限/缓冲）
- 子对象：眼特征提取（EyeDataExtractor）、双路 landmark 处理器、双 OneEuro 簇（fcn.18017a260/18017b600）
- 视线角钳位 ±85°/±89.99°（常量池）

**OneEuroFilter@FilterLib**：非多态内联（无独立 vtable），参数随宿主块传入（FaceTracker 的 0.99/0.65/0.55 簇、GazeDirector 的 0.04/0.05 簇即其 cutoff/beta 风格值）。复刻时直接用 `freq=30, mincutoff∈[0.025,0.05], beta∈[0.5,1.0], dcutoff=1.0` 起步即可对齐手感。

**ConsistencyFilter@ADPC**（std::_Ref_count_obj2 引用）：ActiveDirector（自动取景）一致性滤波，与 AD_GENCROP/AD_UP 阶段配合。

### 3.3 vcam 层状态机

- 潜在初始化（latent init）超时：一般 **3000ms**，StageLight/PreBake 类 **15000ms**（fcn.1800053f0）。
- 每帧日志态：`PC scenario | EnableAiEffects | IsOccluded | isAwaitingPreBake | isPerfMitigated`。
- XVP 预/后处理（ConfigurePreprocessingXVP / PostprocessingXVP）做色彩/旋转归一。
- 元数据透传 `MF_CAPTURE_METADATA_FACEROIS` + Studio FACEMETADATA 三 GUID。

## 4. 对 xrun-rs 的建议（按用户可感知价值排序）

1. **背景虚化强度/模式**（最值得做）：
   - `--bg-mode off|blur|shallow`（对应 KS BLUR=1/SHALLOWFOCUS=4）
   - `--bg-strength 0..1`：实现 = mask EMA α（默认 0.3）与 bokeh 核（BLUR 4.0 / SHALLOW 3.0）线性缩放
   - `--bg-reset-threshold 0.02`：场景切换响应 vs 闪动的折中，**Windows 默认 0.02 是手感基准**
   - `--seg-wfov 0|1`（id109 双分支开关）
2. **眼神接触（EyeContact）**：
   - `--eye-contact off|on|stare`（KS ON/STARE 语义：stare = 更强凝视档）
   - `--eye-contact-version 2`（GAN 路线门控，唯一有效值）
   - id109 的 mask 反馈环必须复刻（第 4 通道 = 上一帧输出，首帧置 0），否则时序稳定性出不来
3. **自动取景**：
   - `--framing none|auto|face|full-frame`（`ActiveDirector.RefinerMode` 枚举直译）
   - `--framing-kind window|cinematic`（Studio AUTOMATICFRAMINGKIND）
   - FaceTracker 参数 `--min-face 30.0 --track-conf 0.99 --detect-conf 0.65/0.55` 可暴露给进阶用户
4. **人脸超分**：`--superres none|1x|2x|4x`（id132，默认 none）
5. **检测阈值**：`--face-threshold 0..1`（默认建议 0.5；Windows 未给出运行默认、读取点缺省 0）
6. **性能旋钮**：`--num-faces 1`、`--threads N`、`--framerate-throttle on|off`、PreBake 语义可映射成"预热"
7. **不建议暴露**：CreativeFilter（本 SKU 无模型，暴露即空转）；StageLight（依赖 PreBake 光照数据，Linux 侧无来源）

参数文件建议采用与 Windows 同名的扁平键（`FaceDetection.FaceThreshold` 等），既对齐文档又便于对照调试。

## 5. 附：关键地址速查（PerceptionCore.dll，imagebase 0x180000000）

| 地址 | 内容 |
|---|---|
| 0x180025590 | 属性注册表（所有 JSON 键 + FloatRangeProp min/max）|
| 0x18002a9d0 | 模型名→枚举 resolver（FACE_LANDMARKS_28=0…320=5）|
| 0x18002c610 | 通用模型加载器（resId → FindResourceW）|
| 0x18002cb55/…cd2c/…e7dc/…e5c7/…df32 | res 101/109/113/119/132 加载点 |
| 0x180037ea0 | id109 normed_rgbm 输入装配（RGB+上帧mask）|
| 0x1800396f0 | ImageTemporalSmoother::addImage（EMA+RMS重置）|
| 0x180014d40 | SegmentationMaskRefiner 构造+process |
| 0x180015170 | ActiveBokehEffect 处理（BLUR/SHALLOW 参数）|
| 0x18013c4f0 | GazeDirector 构造 |
| 0x1801ecb2c-0x1801ece44 | 全局浮点常量池 |

vcam（WindowsEffectsCameraMediaSource.dll）：
| 地址 | 内容 |
|---|---|
| 0x180026dc0 | IKsControl::KsProperty 分发器（跳转表 0x18002b570）|
| 0xdbd90 / 0xde690 | ExtendedCameraControl / WindowsCameraEffect GUID |
| 0x1800053f0 | SetEffectScenarios（3000/15000ms 超时、bit17→0x10）|
| 0x1800819e6 | FSMEnableMsEffects 注册表读取 |

## 参考

- [Extended Camera Controls — Microsoft Learn](https://learn.microsoft.com/en-us/windows-hardware/drivers/stream/standardized-extended-controls-)
- [microsoft/Windows-Camera — WindowsStudio 样例 KsHelper.cs](https://github.com/microsoft/Windows-Camera/blob/master/Samples/WindowsStudio/WindowsStudioSample_WinUI/KsHelper.cs)
- 本仓库 `camera/mep-models/README.md`（模型提取与张量规格）
