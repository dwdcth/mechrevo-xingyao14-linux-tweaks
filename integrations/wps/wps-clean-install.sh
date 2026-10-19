#!/usr/bin/env bash
# wps-clean-install.sh — WPS Office (Arch 系 wps-office-cn) 流氓进程治理一键安装
# 功能：
#   ① NoExtract 永久禁止 EverythingDaemon 落地（全盘索引守护，曾把 FUSE 挂载卡成 D 状态导致挂起失败）
#   ② 安装 wps-launch 包裹器 + 用户级 desktop 覆盖：退出 WPS 后自动清掉常驻 scope
# 卸载：
#   rm -f ~/.local/bin/wps-launch ~/.local/share/applications/wps-office-*.desktop
#   并删掉 /etc/pacman.conf 里那一行 NoExtract
set -euo pipefail

if [ "$EUID" -ne 0 ]; then exec sudo "$0" "$@"; fi
REAL_USER=${SUDO_USER:-$(id -un 1000)}
HOME_DIR=$(getent passwd "$REAL_USER" | cut -d: -f6)
echo ">> 目标用户: $REAL_USER ($HOME_DIR)"

# ---------- ① 禁止 EverythingDaemon ----------
if [ -f /usr/lib/office6/EverythingDaemon ]; then
    if ! grep -q 'usr/lib/office6/EverythingDaemon' /etc/pacman.conf; then
        printf '\n# WPS 全盘索引守护（卡死过 FUSE 挂载/系统挂起，禁用）\nNoExtract   = usr/lib/office6/EverythingDaemon\n' >> /etc/pacman.conf
    fi
    rm -f /usr/lib/office6/EverythingDaemon
    echo ">> EverythingDaemon 已禁用（NoExtract + 删除二进制）"
else
    echo ">> 未发现 EverythingDaemon（可能已处理过）"
fi

# ---------- ② wps-launch 包裹器 ----------
install -Dm755 -o "$REAL_USER" /dev/stdin "$HOME_DIR/.local/bin/wps-launch" <<'WRAPPER'
#!/usr/bin/env bash
# wps-launch — WPS 退出自动清场包裹器
# 用法: wps-launch <真实二进制> [参数...]
# 不依赖包裹进程返回（WPS 多实例会交接秒退），而是全局盯文档进程，
# 连续 60 秒一个都没有 → 停掉 WPS 常驻 scope（wpsoffice/wpscloudsvr/CEF 一起清）。
set -u
BIN=$1; shift
"$BIN" "$@" &

PAT='wps|wpp|et|wpspdf'
for _ in $(seq 15); do
    pgrep -x "$PAT" >/dev/null && break
    sleep 1
done
pgrep -x "$PAT" >/dev/null || exit 0

n=0
while :; do
    sleep 10
    if pgrep -x "$PAT" >/dev/null; then
        n=0
    else
        n=$((n+1))
        [ "$n" -ge 6 ] && break
    fi
done

u=$(systemctl --user list-units 'app-wps*' --no-legend 2>/dev/null | awk '{print $1}')
[ -n "$u" ] && systemctl --user stop $u
exit 0
WRAPPER

# ---------- ③ 用户级 desktop 覆盖（家目录，WPS 更新冲不掉） ----------
mkdir -p "$HOME_DIR/.local/share/applications"
found=0
for f in "$HOME_DIR"/.local/share/applications/wps-office-*.desktop; do rm -f "$f"; done 2>/dev/null || true
for src in /usr/share/applications/wps-office-*.desktop; do
    [ -f "$src" ] || continue
    found=1
    sed -E 's#^Exec=/usr/bin/(wps|et|wpp|wpspdf)#Exec='"$HOME_DIR"'/.local/bin/wps-launch /usr/bin/\1#' \
        "$src" > "$HOME_DIR/.local/share/applications/$(basename "$src")"
done
chown -R "$REAL_USER" "$HOME_DIR/.local/share/applications" "$HOME_DIR/.local/bin/wps-launch"

if [ "$found" = 1 ]; then
    command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 --noincremental 2>/dev/null || true
    command -v kbuildsycoca  >/dev/null && kbuildsycoca  --noincremental 2>/dev/null || true
    echo ">> desktop 覆盖完成（5 个入口走包裹器），菜单缓存已刷新"
else
    echo "!! 未找到 /usr/share/applications/wps-office-*.desktop，跳过 desktop 覆盖"
fi

echo
echo "全部完成。验证：从菜单/Dolphin 打开 WPS 文档 → 关闭所有窗口 → 约 70 秒后："
echo "  ps -eo comm | grep -iE 'office6|wps'   # 应为空"
echo "  systemctl --user list-units 'app-wps*' # 应无输出"
