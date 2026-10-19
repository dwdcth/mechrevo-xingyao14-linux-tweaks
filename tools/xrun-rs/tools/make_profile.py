#!/usr/bin/env python3
# make_profile.py — 从目标 .so 实测生成 xrun-rs ABI profile（原子包：build-id + 符号 + 槽位）
# 用法: python3 tools/make_profile.py <libvitis-ai-runtime.so> [输出.toml（默认打印到 stdout）]
#
# 三步实测，任一步失败立即报错退出（fail-fast，绝不生成半截 profile）：
#   1) build-id: 解析 .note.gnu.build-id（等价 readelf -n）
#   2) 符号: 固定清单逐个在 dynsym 核对存在
#   3) 槽位: 定位 _ZTV11GraphRunner / _ZTVN4vart24CpuFlatTensorBufferOwnedE 两个 vtable，
#      逐槽解析重定位（R_X86_64_RELATIVE 的 addend 反查符号 / R_X86_64_64 直读符号名），
#      按 demangled 函数名匹配角色（execute_async/wait/get_inputs/...）
"""xrun-rs profile generator (fail-fast)."""
import re
import subprocess
import sys

REQUIRED_SYMBOLS = {
    "graph_deserialize": "_ZN3xir5Graph11deserializeERKNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEE",
    "graph_root": "_ZNK3xir8GraphImp17get_root_subgraphEv",
    "sg_has_attr": "_ZNK3xir11SubgraphImp8has_attrERKNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEE",
    "sg_get_attr": "_ZNK3xir11SubgraphImp8get_attrERKNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEE",
    "sg_children": "_ZNK3xir11SubgraphImp25children_topological_sortEv",
    "sg_name": "_ZNK3xir11SubgraphImp8get_nameB5cxx11Ev",
    "t_shape": "_ZNK3xir9TensorImp9get_shapeEv",
    "t_elems": "_ZNK3xir9TensorImp15get_element_numEv",
    "t_dtype": "_ZNK3xir9TensorImp13get_data_typeEv",
    "t_bits": "_ZNK3xir9TensorImp13get_bit_widthEv",
    "attrs_create": "_ZN3xir5Attrs6createEv",
    "attrs_set_attr": "_ZN3xir8AttrsImp8set_attrERKNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEERKSt3any",
    "runner_create_attrs": "_ZN4vart6Runner24create_runner_with_attrsEPKN3xir8SubgraphEPNS1_5AttrsE",
    "map_init_ctor": "_ZNSt3mapINSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEES5_St4lessIS5_ESaISt4pairIKS5_S5_EEEC2ESt16initializer_listISA_ERKS7_RKSB_",
    "any_manager_mapss": "_ZNSt3any17_Manager_externalISt3mapINSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEES7_St4lessIS7_ESaISt4pairIKS7_S7_EEEE9_S_manageENS_3_OpEPKS_PNS_4_ArgE",
    "any_cast_str": "_ZSt8any_castINSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEEEET_RKSt3any",
}

# 角色 → (vtable 符号, demangled 匹配子串)。槽位 = 子串命中的最小槽号。
ROLE_TABLE = {
    "runner_execute_async": ("_ZTV11GraphRunner", "GraphRunner::execute_async"),
    "runner_wait": ("_ZTV11GraphRunner", "GraphRunner::wait"),
    "runner_get_input_tensors": ("_ZTV11GraphRunner", "GraphRunner::get_input_tensors"),
    "runner_get_output_tensors": ("_ZTV11GraphRunner", "GraphRunner::get_output_tensors"),
    "runner_get_inputs": ("_ZTV11GraphRunner", "GraphRunner::get_inputs"),
    "runner_get_outputs": ("_ZTV11GraphRunner", "GraphRunner::get_outputs"),
    "tb_data": ("_ZTVN4vart24CpuFlatTensorBufferOwnedE", "::data("),
    "tb_sync_for_read": ("_ZTVN4vart24CpuFlatTensorBufferOwnedE", "::sync_for_read"),
    "tb_sync_for_write": ("_ZTVN4vart24CpuFlatTensorBufferOwnedE", "::sync_for_write"),
}


def die(msg):
    print(f"[make_profile] FAIL: {msg}", file=sys.stderr)
    sys.exit(2)


def build_id(so):
    out = subprocess.run(["readelf", "-n", so], capture_output=True, text=True).stdout
    m = re.search(r"Build ID:\s*([0-9a-f]+)", out)
    if not m:
        die(f"{so} 无 GNU build-id note（readelf -n）")
    return m.group(1)


def addr2name_map(so):
    m = {}
    for extra in ([], ["-D"]):
        out = subprocess.run(["nm", "--defined-only"] + extra + [so], capture_output=True, text=True).stdout
        for line in out.splitlines():
            f = line.split()
            if len(f) == 3 and f[1] in "TtWwDdBbRrVv":
                m.setdefault(int(f[0], 16), f[2])
    return m


def vtable_slots(so, vt_sym, a2n, max_slots=40):
    """返回 {slot: demangled_name}；vptr = vtable+0x10，槽号从 0 数。"""
    out = subprocess.run(["nm", "-D", "--defined-only", so], capture_output=True, text=True).stdout
    vt = None
    for line in out.splitlines():
        f = line.split()
        if len(f) == 3 and f[2] == vt_sym:
            vt = int(f[0], 16)
            break
    if vt is None:
        die(f"dynsym 里找不到 vtable 符号 {vt_sym}")
    relocs = {}
    for line in subprocess.run(["readelf", "-rW", so], capture_output=True, text=True).stdout.splitlines():
        f = line.split()
        if len(f) >= 4 and f[2] == "R_X86_64_RELATIVE":
            relocs[int(f[0], 16)] = ("rel", int(f[3], 16), None)
        elif len(f) >= 5 and f[2] == "R_X86_64_64":
            relocs[int(f[0], 16)] = ("abs", 0, f[4].split("@")[0])
    slots = {}
    for k in range(max_slots):
        e = relocs.get(vt + 0x10 + 8 * k)
        if e is None:
            break
        name = a2n.get(e[1]) if e[0] == "rel" else e[2]
        if not name:
            break
        d = subprocess.run(["c++filt", name], capture_output=True, text=True).stdout.strip()
        slots[k] = d
    return slots


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    so = sys.argv[1]
    bid = build_id(so)

    dyn = subprocess.run(["nm", "-D", "--defined-only", so], capture_output=True, text=True).stdout
    defined = {line.split(None, 2)[2] for line in dyn.splitlines() if len(line.split(None, 2)) == 3}
    for key, mangled in REQUIRED_SYMBOLS.items():
        if mangled not in defined:
            die(f"符号缺失: {key} = {mangled}")

    a2n = addr2name_map(so)
    measured = {}
    for role, (vt_sym, marker) in ROLE_TABLE.items():
        sl = vtable_slots(so, vt_sym, a2n)
        hit = [k for k, d in sorted(sl.items()) if marker in d]
        if not hit:
            die(f"槽位角色 {role} 在 {vt_sym} 中未命中（marker={marker!r}）；"
                f"用 tools/vt_slots.py 人工核对: {sl}")
        measured[role] = hit[0]

    lines = [
        "# xrun-rs ABI profile — 由 tools/make_profile.py 实测生成（原子包：指纹+符号+槽位）",
        f"# 源: {so}",
        "",
        "[fingerprint]",
        'target_lib = "libvitis-ai-runtime.so"',
        f'build_id = "{bid}"',
        "",
        "[slots]",
    ]
    lines += [f"{k} = {measured[k]}" for k in ROLE_TABLE]
    lines += ["", "[symbols]"]
    lines += [f'{k} = "{v}"' for k, v in REQUIRED_SYMBOLS.items()]
    text = "\n".join(lines) + "\n"

    if len(sys.argv) > 2:
        with open(sys.argv[2], "w") as f:
            f.write(text)
        print(f"[make_profile] OK → {sys.argv[2]} (build_id={bid})")
    else:
        sys.stdout.write(text)


if __name__ == "__main__":
    main()
