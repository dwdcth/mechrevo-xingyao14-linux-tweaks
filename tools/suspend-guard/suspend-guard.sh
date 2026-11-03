#!/usr/bin/env bash
# suspend-guard —— 挂起保险丝(防止"睡死")
#
# 背景: D 状态(不可中断 IO 等待)进程无法被挂起冻结器冻结。systemd-sleep 在
#   user.slice 冻结 60s 超时后仍强行进入内核挂起,内核再冻结失败中止;若中止窗口
#   内恰有唤醒事件(开盖)则机器永久卡死 → 只能强制断电。
#   2026-10-24(QLC 盘读停顿, SiYuan 三进程)与 10-25(云盘 FUSE 守护挂死
#   ——rclone mount / WebDAV / qrypt 之类——find 卡 13.5h)两次事故均为此链路,
#   详见 docs/notes/suspend-freeze-deadlock.md
#
# 本脚本挂在 systemd-suspend.service 等的 ExecStartPre:
#   有持续 D 状态任务 → 挂起服务直接失败(结果=没睡成,系统保持运行,按键唤屏,
#   不再进冻结/中止竞态),并桌面弹窗点名卡死进程。
#
# 安装:
#   sudo install -m755 suspend-guard.sh /usr/local/sbin/suspend-guard
#   sudo systemctl enable 挂三类睡眠服务的 drop-in(见 README.md)
# 手动演示(弹窗+退出码1,不真挂起):
#   SUSPEND_GUARD_DEMO=1 /usr/local/sbin/suspend-guard

set -u

# ---- 配置(按机器定制) ----
NOTIFY_USER="banshan"            # 弹窗接收用户
NOTIFY_UID="1000"
RECHECK_SEC=2                    # D 状态持续超过该秒数才判卡死(防瞬时 IO 误伤)
RATE_LIMIT_SEC=90                # 弹窗最小间隔(防挂起重试风暴刷屏)
STATE_DIR="/run/suspend-guard"   # 限频状态(tmpfs,重启自清)

DEMO="${SUSPEND_GUARD_DEMO:-0}"

log() { echo "suspend-guard: $*"; }

# 采样全部 D 状态的用户态进程,输出行: PID|comm|cmdline前72字符
# 内核线程 /proc/PID/cmdline 为空 → 跳过(它们不是用户态冻结阻碍)
d_snapshot() {
    local state pid comm cmd
    while read -r state pid; do
        [ -r "/proc/$pid/cmdline" ] || continue
        cmd=$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)
        [ -n "$cmd" ] || continue
        comm=$(cat "/proc/$pid/comm" 2>/dev/null) || continue
        echo "$pid|$comm|${cmd:0:72}"
    done < <(ps -eo state=,pid= | awk '$1 ~ /^D/ {print $1, $2}')
}

# 桌面弹窗(带限频;会话不在时静默降级,日志里仍有全量信息)
# 注: root(systemd ExecStartPre)走 runuser;普通用户手动运行时直连会话总线
notify_user() {
    local now last envs
    mkdir -p "$STATE_DIR" 2>/dev/null || STATE_DIR="/tmp/suspend-guard-${UID:-0}"
    mkdir -p "$STATE_DIR" 2>/dev/null || return 0
    now=$(date +%s)
    last=$(cat "$STATE_DIR/last-notify" 2>/dev/null || echo 0)
    [ $((now - last)) -lt "$RATE_LIMIT_SEC" ] && return 0
    echo "$now" > "$STATE_DIR/last-notify" || true
    envs=(XDG_RUNTIME_DIR="/run/user/$NOTIFY_UID"
          DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$NOTIFY_UID/bus")
    if [ "$(id -u)" = "0" ]; then
        runuser -u "$NOTIFY_USER" -- env "${envs[@]}" \
            notify-send -u critical -t 30000 -a suspend-guard -i dialog-warning \
            "$1" "$2" >/dev/null 2>&1 || true
    else
        env "${envs[@]}" \
            notify-send -u critical -t 30000 -a suspend-guard -i dialog-warning \
            "$1" "$2" >/dev/null 2>&1 || true
    fi
}

first="$(d_snapshot)"

if [ "$DEMO" != "1" ]; then
    if [ -z "$first" ]; then
        log "PASS — 无 D 状态任务,放行挂起"
        exit 0
    fi
    sleep "$RECHECK_SEC"
    second="$(d_snapshot)"
    if [ -z "$second" ]; then
        log "PASS — 瞬时 D 已消散,放行挂起"
        exit 0
    fi
    # 两轮快照都处于 D 的 PID 交集 = 持续卡死
    stuck_pids=$(comm -12 <(printf '%s\n' "$first" | cut -d'|' -f1 | sort -u) \
                         <(printf '%s\n' "$second" | cut -d'|' -f1 | sort -u))
    if [ -z "$stuck_pids" ]; then
        log "PASS — D 状态任务未持续,放行挂起"
        exit 0
    fi
fi

# ---- 拦截: 组装点名名单 ----
if [ "$DEMO" = "1" ]; then
    total=1
    details="• PID 123456 find
  find /home/banshan -name '*.md'   (演示数据)"
else
    total=0
    details=""
    for pid in $stuck_pids; do
        line=$(printf '%s\n' "$first" | grep -m1 "^${pid}|") || continue
        total=$((total + 1))
        # 最多列 6 条,避免弹窗爆版
        if [ "$total" -le 6 ]; then
            details="${details}• PID $(printf '%s' "$line" | cut -d'|' -f1) $(printf '%s' "$line" | cut -d'|' -f2)
  $(printf '%s' "$line" | cut -d'|' -f3-)
"
        fi
    done
    [ "$total" -gt 6 ] && details="${details}… 其余 ${total} 条见 journalctl -u systemd-suspend.service"
fi

log "ABORT — 检测到 ${total} 个持续 D 状态进程,中止挂起(强行挂起会睡死):"
printf '%s\n' "$details" | sed 's/^/  /'

notify_user \
    "已拦截挂起: ${total} 个进程卡死(D 状态)" \
    "${details}
处理: kill -9 上述 PID,或重启对应服务(如云盘 FUSE 挂载服务)
确认后重新合盖即可。日志: journalctl -u systemd-suspend.service"

exit 1
