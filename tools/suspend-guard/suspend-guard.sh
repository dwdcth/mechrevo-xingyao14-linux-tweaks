#!/usr/bin/env bash
# suspend-guard —— 挂起保险丝(防"睡死",兼防"幽灵盖子"挂起循环)
#
# 背景 A: D 状态(不可中断 IO 等待)任务无法被挂起冻结器冻结。systemd-sleep 在
#   user.slice 冻结 60s 超时后仍强行进入内核挂起,内核再冻结失败中止;若中止窗口
#   内恰有唤醒事件(开盖)则机器永久卡死 → 只能强制断电。
#   2026-10-24(QLC 盘读停顿)、10-25 与 11-05(云盘 FUSE 守护挂死——rclone mount、
#   WebDAV、qrypt 之类)三次事故均为此链路,
#   详见 docs/notes/suspend-freeze-deadlock.md
#
# 背景 B: EC 盖子状态位卡死在"闭合"(物理开盖)。logind 把假闭合当启动初始值,
#   唤醒后每 30s(HoldoffTimeoutSeconds)复查仍"闭合"→ 反复发起挂起,体感
#   "反复注销";开机 30s 内的首挂还会把登录界面 compositor 打成黑屏。
#   2026-11-04 一次,详见 docs/notes/lid-stuck-suspend-loop.md
#
# 本脚本挂在 systemd-suspend.service 等的 ExecStartPre,只在睡眠请求瞬间运行
# (零常驻、不监听任何事件):
#   检查 1 幽灵盖子: ACPI lid=闭合,但本 boot 最后一条 lid 事件不是 "Lid closed."
#     (真实合盖必有翻转事件;EC 卡死是初始值,不产生事件)→ 中止挂起,弹窗指引
#     "断电复位 EC"。journal 不可用时放行(fail-open)。
#   检查 2 线程级 D 状态: 内核冻结器按"线程"冻结,ps -e 只列主线程——"主线程
#     健康、工作线程卡 D"的 FUSE 守护完全不可见(11-05 保险丝三次假阴性放行的
#     根因)。两轮全线程快照交集仍有 D 线程时:
#       a. 先自动清理——kill -9 卡死线程的主进程。SIGKILL 会关闭 /dev/fuse,
#          内核立即 abort 在途 FUSE 请求,阻塞在页缓存读上的 D 线程随之释放;
#          清干净则放行挂起(盒盖→杀卡死→照常入睡)。
#       b. 杀不动(D 在真实硬件 IO 上,如 QLC 读停顿)→ 中止挂起并弹窗点名,
#          "睡死强制断电"降级为"没睡成、系统保持醒着"。
#
# 安装:
#   sudo install -m755 suspend-guard.sh /usr/local/sbin/suspend-guard
#   sudo systemctl enable 挂三类睡眠服务的 drop-in(见本目录 README.md)
# 手动演示(弹窗+退出码1,不真挂起、不 kill 任何进程):
#   SUSPEND_GUARD_DEMO=1 /usr/local/sbin/suspend-guard      # D 状态线程拦截演示
#   SUSPEND_GUARD_DEMO_LID=1 /usr/local/sbin/suspend-guard  # 幽灵盖子拦截演示

set -u

# ---- 配置(按机器定制) ----
NOTIFY_USER="banshan"            # 弹窗接收用户
NOTIFY_UID="1000"
RECHECK_SEC=2                    # D 状态持续超过该秒数才判卡死(防瞬时 IO 误伤)
KILL_WAIT_SEC=5                  # 自动清理后等待 D 线程退出再复查
KILL_PROTECT_MIN_TGID=1000       # TGID 小于该值不自动 kill(系统进程保护网)
RATE_LIMIT_SEC=90                # 弹窗最小间隔(防挂起重试风暴刷屏)
STATE_DIR="/run/suspend-guard"   # 限频状态(tmpfs,重启自清)
LID_SKIP_STAMP="/run/suspend-guard/skip-lid"  # touch 此文件临时停用幽灵盖子检查

DEMO="${SUSPEND_GUARD_DEMO:-0}"
DEMO_LID="${SUSPEND_GUARD_DEMO_LID:-0}"

log() { echo "suspend-guard: $*"; }

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

# ---- 检查 1: 幽灵盖子 ----
# 返回 0 = 判定幽灵闭合(拦截);1 = 正常/不适用。
# 判据: ACPI lid 当前= closed,但本 boot 最后一条 lid 事件不是 "Lid closed."
# (含"从无事件"——真实合盖必有翻转事件,EC 卡死闭合是 logind 启动时的初始值,
# 不产生任何事件)。已知误拦面: 盖子合着开机(翻盖外接屏)会被拦,
# 此时 touch "$LID_SKIP_STAMP" 绕过。
lid_phantom() {
    local f state evs last_ev
    [ -e "$LID_SKIP_STAMP" ] && return 1
    f=$(ls /proc/acpi/button/lid/LID*/state 2>/dev/null | head -n1)
    [ -n "$f" ] || return 1                     # 无盖子(台式机/虚拟机)
    state=$(awk '{print $2}' "$f" 2>/dev/null)
    [ "$state" = "closed" ] || return 1         # 盖子开着 → 与本检查无关
    evs=$(journalctl -b -u systemd-logind -o cat --no-pager 2>/dev/null) || return 1
    last_ev=$(printf '%s\n' "$evs" | grep -a '^Lid ' | tail -n1)
    [ "$last_ev" = "Lid closed." ] && return 1  # 最近的翻转就是合盖 → 真合盖
    return 0                                    # 无事件/最后是开盖 → 假闭合
}

if [ "$DEMO_LID" = "1" ] || lid_phantom; then
    log "ABORT — 幽灵盖子: ACPI lid=闭合,但本次开机没有对应的合盖事件(EC 状态位卡死)"
    log "  放行后果: logind 每 30s 重试挂起(体感: 反复注销/黑屏),greeter 可能被打残"
    notify_user \
        "已拦截挂起: 盖子状态疑似 EC 卡死(假闭合)" \
        "ACPI 上报盖子=闭合,但本次开机没有合盖事件 → EC 状态位卡死。
处理: 保存工作后关机并断电 ≥30 秒复位 EC(温重启无效)。
临时继续用可 touch $LID_SKIP_STAMP 绕过本检查。"
    exit 1
fi

# ---- 检查 2: 线程级 D 状态 ----
# 采样全部线程的 D 状态(必须含非 leader 线程): 内核冻结器按线程冻结,
# "主线程健康、工作线程卡 D"的 FUSE 守护对 ps -e 级(仅主线程)采样完全不可见。
# 内核线程 /proc 无 cmdline → 跳过(它们不是用户态冻结阻碍)。
# 输出行: TID|TGID|线程comm|主进程cmdline前72字符
d_snapshot() {
    local state tgid tid comm cmd
    while read -r state tgid tid; do
        comm=$(cat "/proc/$tgid/task/$tid/comm" 2>/dev/null) || continue
        cmd=$(tr '\0' ' ' < "/proc/$tgid/cmdline" 2>/dev/null)
        [ -n "$cmd" ] || continue
        echo "$tid|$tgid|$comm|${cmd:0:72}"
    done < <(ps -eLo state=,pid=,tid= | awk '$1 ~ /^D/ {print $1, $2, $3}')
}

first="$(d_snapshot)"

if [ "$DEMO" != "1" ]; then
    if [ -z "$first" ]; then
        log "PASS — 无 D 状态线程,放行挂起"
        exit 0
    fi
    sleep "$RECHECK_SEC"
    second="$(d_snapshot)"
    if [ -z "$second" ]; then
        log "PASS — 瞬时 D 已消散,放行挂起"
        exit 0
    fi
    # 两轮快照都处于 D 的线程 TID 交集 = 持续卡死
    stuck_tids=$(comm -12 <(printf '%s\n' "$first" | cut -d'|' -f1 | sort -u) \
                          <(printf '%s\n' "$second" | cut -d'|' -f1 | sort -u))
    if [ -z "$stuck_tids" ]; then
        log "PASS — D 状态线程未持续,放行挂起"
        exit 0
    fi

    # ---- 自动清理: kill 持续卡死线程的主进程 ----
    # SIGKILL 会关闭 /dev/fuse → 内核 abort 在途 FUSE 请求 → 页缓存上的
    # D 等待随之释放,绝大多数云盘 FUSE 类卡死可被根治,挂起得以继续。
    # TGID < KILL_PROTECT_MIN_TGID(系统进程)不杀,交给下面的拦截路径点名。
    killed="" kcount=0
    declare -A seen_tgid=()
    for tid in $stuck_tids; do
        line=$(printf '%s\n' "$first" | grep -m1 "^${tid}|") || continue
        tgid=$(printf '%s' "$line" | cut -d'|' -f2)
        case "$tgid" in ''|*[!0-9]*) continue ;; esac
        [ "$tgid" -ge "$KILL_PROTECT_MIN_TGID" ] || continue
        [ -n "${seen_tgid[$tgid]:-}" ] && continue
        seen_tgid[$tgid]=1
        if kill -9 "$tgid" 2>/dev/null; then
            kcount=$((kcount + 1))
            killed="${killed}• 线程 $tid $(printf '%s' "$line" | cut -d'|' -f3) → kill -9 $tgid
"
        fi
    done
    if [ "$kcount" -gt 0 ]; then
        log "自动清理: 已对 ${kcount} 个卡死进程发出 SIGKILL,等待 ${KILL_WAIT_SEC}s 复查:"
        printf '%s' "$killed" | sed 's/^/  /'
        notify_user \
            "挂起前自动清理: kill -9 了 ${kcount} 个卡死进程" \
            "${killed}
云盘 FUSE 守护卡死可被 SIGKILL 根治(连接关闭 → 内核 abort 在途请求)。
若挂起仍失败,见 journalctl -u systemd-suspend.service 点名幸存者。"
        sleep "$KILL_WAIT_SEC"
        first="$(d_snapshot)"
        if [ -z "$first" ]; then
            log "PASS — 清理后无 D 状态线程,放行挂起"
            exit 0
        fi
        stuck_tids=$(printf '%s\n' "$first" | cut -d'|' -f1 | sort -u)
    fi
fi

# ---- 拦截: 组装点名名单 ----
if [ "$DEMO" = "1" ]; then
    total=1
    details="• 线程 123456 fuse-rd (主进程 123400 云盘守护)
  /opt/example-fused --mountpoint /mnt/cloud   (演示数据)"
else
    total=0
    details=""
    for tid in $stuck_tids; do
        line=$(printf '%s\n' "$first" | grep -m1 "^${tid}|") || continue
        total=$((total + 1))
        # 最多列 6 条,避免弹窗爆版
        if [ "$total" -le 6 ]; then
            details="${details}• 线程 $(printf '%s' "$line" | cut -d'|' -f1) $(printf '%s' "$line" | cut -d'|' -f3) (主进程 $(printf '%s' "$line" | cut -d'|' -f2),kill 无效)
  $(printf '%s' "$line" | cut -d'|' -f4-)
"
        fi
    done
    [ "$total" -gt 6 ] && details="${details}… 其余 ${total} 条见 journalctl -u systemd-suspend.service"
fi

log "ABORT — ${total} 个线程卡死且 kill 无效(D 在硬件 IO 上),中止挂起防睡死:"
printf '%s\n' "$details" | sed 's/^/  /'

notify_user \
    "已拦截挂起: ${total} 个线程卡死且 kill 无效" \
    "${details}
这些进程的 D 状态在真实硬件 IO 上,kill 不动;强行挂起会睡死。
处理: 等待 IO 自行恢复,或重启对应服务/保存工作后重启。
日志: journalctl -u systemd-suspend.service"

exit 1
