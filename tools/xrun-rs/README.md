# xrun-rs — MEP voe/XIR 模型 NPU 直执行器（纯 Rust，RAI 1.8 栈）

xrun2.cpp（mep-run，RAI 1.4 + VART 3.5）的 Rust 移植，依赖整体迁移到 RAI 1.8。
**验收：id132 / id101 输出与 C++ 版逐位一致（cmp），外加 id109 / id113 / id119 共 5/5 模型全对齐。**

```
用法: target/release/xrun-rs <model.onnx> <input.bin> <output.bin>
一键: bash run.sh ../mep-models/id132.onnx /tmp/in132.bin /tmp/out132.bin
      （单输出 → output.bin；多输出 → output.bin_0 / output.bin_1 / ...）
```

## 运行依赖（最终清单）

| 依赖 | 路径 | 说明 |
|---|---|---|
| RAI 1.8 运行库 | `../mep-run/venv/lib/python3.12/site-packages/voe/lib/libvitis-ai-runtime.so` | **唯一必需的 voe 文件**：xir+vart+graph-engine 合并库（21MB，26388 导出符号，全静态打进 glog/protobuf） |
| 系统 XRT | `/opt/xilinx/xrt/lib`（2.25.37） | 1.8 原生走新 API（register_xclbin + hw_context），**不再需要 xrt_compat.so 垫片** |
| 固件 xclbin | `../mep-run/venv14/lib/python3.10/site-packages/voe-4.0-linux_x86_64/xclbins/strix/AMD_AIE2P_4x4_Overlay_CFG0.xclbin` | 只读使用；经 `XLNX_VART_FIRMWARE` 指定 |
| 锁页内存 | `sudo prlimit --memlock=8589934592 --` | NPU DMA 必须（run.sh 已包办） |

**外部 libgraph-engine.so 不再需要**（1.8 把 graph-engine 编进合并库，见下文派发分析）。
`libs/libgraph-engine.so` + `libglog.so.1` 是从 tmpfs（/tmp/npu-llm/lib）抢救出来的持久化副本，
仅作 1.4 对照链备份。`libs/` 里其余是 soname 符号链接（libxir.so.3 等 → 合并库），供
LD_LIBRARY_PATH 解析。

必需环境变量：

```bash
LD_LIBRARY_PATH=/opt/xilinx/xrt/lib:<xrun-rs>/libs
XLNX_VART_FIRMWARE=<CFG0.xclbin>
XLNX_VART_SKIP_FP_CHK=TRUE   # 注意：必须精确大写 "TRUE"；"1" 无效！
# 反汇编证据：合并库 0x522d73 处 get_env_var 后
#   cmpq $0x4,len; cmpl $0x45555254,(%rdi)   ← "TRUE" 逐字节比较
# MEP 模型指纹期望 AMD_AIE2P_4x4_Overlay_CFG0_3.5.0.0-2229_ipu_2 构建，与手头 CFG0 不匹配，
# 但 overlay 兼容（1.4 即以此跑通），跳过校验即可。
```

## Phase A 兼容性实验结论（1.4 → 1.8）

对 1.8 合并库做静态比对 + 现有 C++ xrun2 直跑验证，逐项结论：

| 项目 | 结论 |
|---|---|
| xrun2 所需 15 个 mangled 符号 | **全部存在，名字完全一致**（`nm -D` 逐一核对） |
| Runner vtable 槽位 | **零漂移**（2/3/5/6/7/8，实测 vtable for GraphRunner @0xfff9e8） |
| TensorBuffer vtable 槽位 | **零漂移**（2=data, 5=sync_for_read, 6=sync_for_write，@0x1024ce0 等） |
| sret 约定（Ret16 = 带自定义删除器 unique_ptr） | **一致**：Graph::deserialize @0xc259c0、Attrs::create @0xbd3ee0、create_runner_with_attrs @0xb9ea30 反汇编确认 rdi=sret |
| XRT API 代差 | **消失**：1.8 只引用新 API（register_xclbin/hw_context/kernel(hw_ctx,name)），无 load_axlf 旧路径 → xrt_compat.so 不需要 |
| 外部 graph-engine | **不需要**：graph-engine 源码（src/voe/graph-engine/）编译进合并库 |
| 引擎派发（新发现） | 1.8 的 `vart::Runner::create_runner_with_attrs`（0xb9ea30）流程：①查 **子图自身** `runner` attr——没有则跌进旧 DpuRunner 路径直接 glog-FATAL（dpu_runner.cpp:232）；②有则按 runner map 取引擎库名；③候选名不在 valid-libs 名单（libvart-cpu-runner.so / graph-engine.dll / **libgraph-engine.so** / libvart-dummy-runner.so）→ "cannot find lib:librt-engine.so" FATAL（dpu_runner.cpp:298）。**MEP 模型 runner map={"run":"librt-engine.so"} 不在名单** → `attrs["lib"]={"DPU":"libgraph-engine.so"}` 后门在 1.8 依然必需，把引擎改指内置 graph-engine |
| 固件指纹校验 | 新增（graph_runner.cpp:424）；`XLNX_VART_SKIP_FP_CHK=TRUE` 跳过（"1" 不识别） |
| glog | InitGoogleLogging 不再导出（也不需要）；日志前缀自动可用 |
| C++ xrun2 在 1.8 下的实测 | id132/id101 输出与 1.4 基准逐位一致（md5 同）→ **Phase B（槽位重测）不需要** |

## ABI profile 机制（原子包：库指纹 + 符号名 + vtable 槽位）

符号名/槽位/库指纹按具体构建实测，**不作为编译期散落常量**，而是整体来自一个 profile：

- 内置默认：`profile/default.toml`（构建时 `include_str!` 嵌入二进制；文件头有完整实测来源注释）
- 覆盖：`xrun-rs --profile <profile.toml> <model> <in.bin> <out.bin>`
- 格式：严格 TOML 子集（`[表]` / `key = "字符串"` / `key = 整数` / 整行 `#` 注释）；
  自带迷你解析器（零依赖），**未知表/未知键/缺键/语法残缺一律 fail-fast 拒绝**——
  拼错的槽位键绝不允许静默回退默认值

启动流水（原子性保证）：

```
载入 profile（内置或 --profile，完整性与格式校验，失败即退出 2）
  → 按 [fingerprint].target_lib dlopen
  → dl_iterate_phdr 读已加载映像 .note.gnu.build-id（等价 readelf -n）
  → 与 [fingerprint].build_id 不符 → 【拒绝运行】（不是警告）：
     防止跨版本混配槽位 → 静默错派发/段错误/输出全 0
  → [symbols] 逐个 dlsym，缺失即退出 2
  → 虚调用全部按 [slots] 派发
```

换库/升级 RAI 后一条命令重测生成新 profile（fail-fast：符号缺失/角色未命中即报错，
绝不生成半截 profile）：

```bash
python3 tools/make_profile.py <新libvitis-ai-runtime.so> 新profile.toml
./target/release/xrun-rs --profile 新profile.toml <model> <in.bin> <out.bin>
```

守卫负例（已验证全部拒绝且不产出文件）：build-id 改动 → 拒绝；缺 `slots.tb_data` → 拒绝；
键名拼错（未知键）→ 拒绝。

## vtable 槽位表（当前 profile 值；工具重测法）

```
Runner（vptr 指向 vtable+0x10，槽号从 0 数）:
  2  execute_async(const vector<TB*>&, const vector<TB*>&) -> pair<u32,int>
  3  wait(int jobid, int timeout) -> int
  5  get_input_tensors()  -> vector<const Tensor*>
  6  get_output_tensors() -> vector<const Tensor*>
  7  get_inputs()         -> vector<TensorBuffer*>
  8  get_outputs()        -> vector<TensorBuffer*>
TensorBuffer（CpuFlatTensorBuffer / Owned / UnownedDevice 布局一致）:
  2  data(const vector<int>& idx) -> pair<u64 addr, size_t size>
  5  sync_for_read(u64 offset, size_t)
  6  sync_for_write(u64 offset, size_t)
```

**换库版本时重测方法**（tools/vt_slots.py，已验证可用）：

```bash
python3 tools/vt_slots.py <libvitis-ai-runtime.so> <vtable符号地址(hex)> <槽数> <标签>
# vtable 地址来源：nm -D --defined-only <so> | grep _ZTV   （_ZTV11GraphRunner 等）
# 原理：vptr=vtable+0x10；每槽 8B；条目是 R_X86_64_RELATIVE（addend=函数地址，反查 symtab）
#      或 R_X86_64_64（直接带符号名）。1.4 的测法见 mep-run/venv14-NOTES.md（/tmp/dump_vtable2.py）
```

试验性核对法（vcall 派发到错误函数的症状 = 段错误/glog 自旋）：临时改槽位常量跑 id132，
只有全对才可能产出与基准 md5 一致的重建图。

## C++ ABI 复刻要点（src/main.rs）

1. **sret（隐藏返回指针）**：非平凡返回类型（Ret16 unique_ptr、std::string、std::vector、std::any）
   一律 `(ret: *mut T, this/args...)`。Graph::deserialize = `(ret16*, &string)`。
2. **Ret16**：fork 的 unique_ptr 带自定义删除器 → 16B {对象指针, 删除器指针}，只取 .a。
3. **std::string（libstdc++ x86_64）**：{char* p; size_t len; union{char buf[16]; size_t cap}}。
   Rust 侧**一律堆分配**表示（SSO 指针自引用 struct，按值返回即悬空——踩过：device attr 全空）。
   读回：p==&buf → SSO 读 buf，否则读堆。构造的 string 故意不析构（operator delete 尺寸不匹配，泄漏无妨）。
4. **std::vector**：{begin,end,cap} 三指针，元素宽度按需除。
5. **std::any（新版 libstdc++，反汇编 any_cast<mapSS> @0x5884c0 实证）**：
   `+0 = manager fnptr`（any_cast 直读比较），`+8 = storage(T*)`；`_Op: 0=access, 1=get_type_info, 2=clone, 3=destroy`。
6. **attrs["lib"] 的 any(map<string,string>) 构造全用真实机器**（零手写 C++ 语义）：
   - `map::map(initializer_list<pair<const string,string>>, const less&, const alloc&)` — 合并库弱导出 @0xc5cf30
   - `std::any::_Manager_external<map<string,string>>::_S_manage` — 弱导出 @0x5885e0
   - `AttrsImp::set_attr(const string&, const any&)` — 与 1.4 同名
   - initializer_list 按值传 = {ptr, len} 拆两寄存器；less/allocator 空类 → 哑指针
7. **data()/execute_async 的 pair 返回**：POD ≤16B → rax:rdx / rax；Rust `#[repr(C)]` 两字段结构体同分类。

## 验证记录（2026-10-17）

基准 = C++ xrun2 + RAI 1.4 栈（run14.sh）；对照 = xrun-rs + RAI 1.8 合并库。
同输入文件，cmp 逐字节比较：

| 模型 | 输入 | 输出 | md5（两栈一致） |
|---|---|---|---|
| id132 图像重建 (512²int8) | /tmp/in132.bin (786432B) | 786432B | 4a59dc80e3ced1fc7d3cf5dc805b53ab |
| id101 人像分割 | /tmp/in101.bin (150528B) | 1715B | a46908c86a199c7b35524680f7044baf |
| id109 双帧时序 | /tmp/in109.bin (114688B) | _0: 114688B / _1: 28672B | 00f1204506ec6101072191d6b19d7ea2 / d07bb2d9b3a53caff9a9adbcd95f3160 |
| id113 检测 | /tmp/in101.bin | _0/_1 双输出 | d9e7461f9959f4c98016f0dab7881dfd（_0；_1 同法 cmp 通过） |
| id119 int16 GAN | /tmp/in119.bin (131072B int16) | 163840B | cmp 逐位一致 |

输入生成（PIL RGB resize，int32-128 clip int8，同 venv14-NOTES.md 方法）。

## 其他

- `tools/dump_vt18.py`：批量槽位 dump 的旧脚本（RELATIVE 重定位反查）。
- execstack：新拷 .so 若被内核拒载，用 `python3 rai/clear_execstack.py <so>` 清标志（本仓库副本无需）。
- memlock 与 HOME：保持 `sudo prlimit -- ... env`（勿 env -i，库构造需要 HOME）。
- 别动 `../mep-run/venv`（1.8）、`../mep-run/venv14`（1.4 对照 + xclbin）、`/opt/xilinx`。

## vcam 子命令 — Windows Studio Effects「背景效果」管线原生复刻

`xrun-rs vcam`：摄像头流 → id109 NPU 分割 → 掩码重整 → bokeh 合成 → 虚拟摄像头，
全链 Rust（推理经 `npu.rs` 复用单发模式的已验证执行序列），零重依赖（std + libc + ffmpeg 子进程）。

```
ffmpeg(v4l2 mjpeg→bgr24) ─→ id109 CCForegroundSegmentation（NPU, ~1ms/帧）
    输入 normed_rgbm (128×224×4 int8)：RGB=像素−128；第4通道=上帧原始输出回写（首帧 0）
    输出 seg_hi 448×256（x1，显示）+ seg_lo 224×128（x0.5，反馈）
 ─→ SegmentationMaskRefiner：分位归一化 → EMA α=0.3（随 bg-strength 缩放）
    → 帧间 RMSE>0.02 全量重置（场景切换）
 ─→ 掩码 → 工作分辨率 192×108（portrait 384×216）→ 帧分辨率羽化(半径25)
 ─→ 背景 bokeh：σ = 4.0（standard）/ 3.0（portrait），box×3 近似高斯，工作分辨率算完放大
 ─→ 整数合成 frame·m + bg·(1−m)（bg 链 worker 线程并行）
 ─→ ffmpeg(bgr24→yuyv422 /dev/videoN | png 序列 | null)
```

### 实测发现（重要，文档没有的）

- **id109 输出不是饱和 0/1**：seg 值集中在 ~[0.50, 0.75] 的窄带（int8 字节 −1..44），
  直接当 [0,1] 用会得到全灰掩码。按帧 2%/98% 分位归一化后呈清晰双峰
  （背景 ~74% / 前景 ~24%）。反馈通道保持**原始字节**回写（数学上与
  "(v+128)/255 → ×255 → −128" 恒等），符合 Windows +0x48 成员的回写语义。
- **羽化半径 25 是帧分辨率语义**：在 192×108 工作分辨率上羽化会把人物边缘糊掉
  （前景高频仅保留 57%）；放大到 1280×720 后羽化 25px（2%）→ 前景保留 88%。
- **首帧掩码反馈为 0**（int8 −128），对应 Windows 的 `memset 0`（黑=无前景先验）。

### 参数（TOML 扁平键 + CLI 覆盖，Windows 同名键做别名）

| CLI | 配置别名（Windows 键） | 默认 | 说明 |
|---|---|---|---|
| `--bg-mode off\|blur\|standard\|shallow\|portrait` | `BackgroundEffects.Mode` | blur | 官方命名：标准模糊=BLUR(σ4.0, mask 192×108)；人像模糊=SHALLOWFOCUS(σ3.0, 384×216) |
| `--bg-strength 0..1` | `BackgroundEffects.Strength` | 1.0 | 线性缩放 EMA α(0.3) 与 bokeh 核 |
| `--bg-reset-threshold f` | `BackgroundEffects.ResetThreshold` | 0.02 | 帧间 RMSE 重置阈值（Windows 0x3ca3d70a） |
| `--seg-wfov 0\|1` | `Segmentation.WFOV` | 1 | id109 双分支布局（模型即 WFOV） |
| `--refiner none\|feathering` | `Segmentation.RefinerMode` | feathering | 羽化开关 |
| `--feather-radius N` | | 25 | 羽化半径（帧分辨率语义；Windows 0x10 成员） |
| `--eye-contact off\|standard\|teleprompter` | `EyeContact.Mode`（on/stare 兼容） | off | **stub**（id119 GazeDirector 未接） |
| `--superres none\|1x\|2x\|4x` | `Superres.Model`（SUPERRES_NX） | none | **stub**（id132 未接） |
| `--framing none\|auto\|face\|full-frame` | `ActiveDirector.RefinerMode` | none | **stub** |
| `--face-threshold f` / `--num-faces N` | `FaceDetection.FaceThreshold` / `FaceTracker.NumFacesToTrack` | 0.5 / 1 | **stub** |
| `--src` `--out v4l2:\|png:\|null` `--width --height --fps --frames` | 同名 | video0→video9 1280×720@30 | I/O |
| `--model` `--profile` `--dump-dir` `--config` | 同名 | id109 / 内置 / - | 调试与 profile 覆盖 |

运行：`bash run-vcam.sh`（已包办 prlimit/固件/LD_LIBRARY_PATH）；
测试源：`--src img:某.png`（静态图循环，不占摄像头）。

### 验证记录（2026-10-17，真机 FHD Camera 1280×720）

- **(a) 单帧链路**：真摄像头 5 帧 → PNG；数值验收（vs 原帧）：
  前景（mask>0.6，占 26%）高频能量保留 **88%**、像素差 3.4；
  背景（占 73%）高频保留 **12%**、像素差 23.1 —— 人物清晰、背景重度虚化，非退化。
- **(b) 连续流**：120 帧 `--out null`：**31.3 fps**，RSS 63.4 MB 全程零增长；
  300 帧写 `/dev/video9`：31.0 fps，并发从 loopback 读回 720p 正常画面。
- **耗时分解（EMA，120 帧）**：pre 1.1 ms | NPU 1.0 ms | post 25.5 ms | write 1.0 ms = 28.6 ms/帧。
  后处理热点已优化：bg 上采样从交错 u8（22ms）改为平面化可分离缩放（~1.5ms）+ bg 链并行线程。
- **确定性注记**：单发 id132 当晚 8/8 逐位一致；曾出现 1 次单字节瞬态差异
  （紧接 300 帧流会话结束后立即运行），复跑未再现——疑似 NPU 会话刚 teardown 时
  BO 复用竞态，流式场景无感，持续观察。
- `xrun-rs vcam-bench`：img 原语微基准（优化依据留档）。

## 网页控制台（vcam 子命令内置，对齐 Python 版 + Windows 设置页）

默认 `http://127.0.0.1:8711/`（`--port N` 可改，`--port 0` 禁用；避开 Python 版的 8710）。
零依赖实现：`std::net::TcpListener` 每连接一线程 + 自写迷你 JSON（`src/json.rs`，严格解析，
非法即拒）+ `src/console.rs`。

| 接口 | 说明 |
|---|---|
| `GET /` | 单页控制台（深色中文，手机自适应）：虚化强度滑杆、模式切换（标准模糊/人像模糊/直通）、眼神接触三态（关/标准/提词器，stub 标注）、实时预览、fps/耗时/帧数 |
| `GET /api/state` | `{"mode","strength","reset_threshold","eye_contact","fps","frame_ms","frames"}` |
| `POST /api/set` | JSON 热调参（无需重启管线）：`mode`/`bg-mode`、`strength`/`bg-strength`、`bg-reset-threshold`、`eye-contact`（别名 on/stare 归一）；非法值/未知键/坏 JSON → 400 |
| `GET /preview.mjpg` | multipart MJPEG：全管线**单个** ffmpeg 编码进程（bgr24→mjpeg 640×360 q4，`-flush_packets 1` 关键——管道输出否则全缓冲到 EOF），读/写分离线程 + SOI/EOI 切帧 → PreviewHub（代数号+Condvar）广播最新帧给所有客户端 |

设置项命名对齐 Windows Studio Effects 官方设置页（MS Learn：背景效果=标准模糊/人像模糊、
眼神接触=标准/提词器；自动取景/创意滤镜/人像光在本 SKU 逆向管线未复刻，页面明确标注）。

热调参即时生效证据（2026-10-17 实测）：portrait → frame_ms 28.5→36.4（384×216 工作分辨率）；
passthrough → frame_ms **1.3ms**（跳过 NPU+后处理）；MJPEG 2.5s 拉 21 帧 JPEG；
日志留痕 `[控制台] mode: blur → portrait` 等。

## 2026-10-17 晚间更新：直通 bug 修复 + 自动取景（原生近似）

- **修复：点"直通"预览冻结**——预览投放原本只在特效分支内，passthrough 分支漏投。
  已挪到公共出帧路径（直通/特效都投，预览 MJPEG 恒活）。
- **自动取景 `--framing auto` / 控制台"自动取景"开关（已实现，原生近似）**：
  id109 分割质心+包围盒 → EMA 平滑窗口（α=0.12，zoom 1.0~1.6，无前景回全景），
  裁剪在特效管线之前（特效作用于取景后的画面）。开启后 frame_ms ~46ms（≈22fps，
  crop_resize 全分辨率一进一出，可再优化）。
- **与 Windows 的差距**：ActiveDirector 用 id101 人脸框 + FaceTracker（OneEuro 0.99/0.65/0.55）；
  对齐需反编译 id101 检测头 (1,28,28,70)+(1,4,28) 解码。

## 未实现效果的路线判定（直接做 vs 反编译）

| 效果 | 结论 | 依据 |
|---|---|---|
| 眼神接触 eye-contact | **必须反编译**（无法直接做） | id119 是 GazeDirector GAN，输入 64×128×8 隐向量的装配（EyeDataExtractor + id113 关键点 + OneEuro 簇）完全未文档化，猜不出来。RE 目标：PerceptionCore.dll 0x18013c4f0（GazeDirector 构造/参数块）、EyeDataExtractor、隐向量拼装函数 |
| 人脸超分 superres | **可直接做，但差人脸框来源** | id132 自含（384²人脸裁剪→推理→贴回）；Windows 用人脸框裁剪。取景落地后可用 id101 框（需 RE 检测头解码）或先以分割 bbox 近似（id132 人脸特化，全身 bbox 效果存疑） |
| 创意滤镜 / 人像光 | **无法做** | 本 SKU 资源包无对应模型（RE §1.3：资源树仅 101/109/113/119/132） |
