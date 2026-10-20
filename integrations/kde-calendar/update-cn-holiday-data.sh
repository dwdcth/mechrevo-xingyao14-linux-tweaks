#!/usr/bin/env bash
# 下载最新中国节假日数据（KHolidays 上游）+ 去英文化 + 重启日历
# 每年 11 月国务院发布次年放假安排、上游数据更新后跑一次
set -euo pipefail

DEST="$HOME/.local/share/kf5/libkholidays/plan2/holiday_cn_zh-cn"
URL='https://invent.kde.org/frameworks/kholidays/-/raw/master/holidays/plan2/holiday_cn_zh-cn'

mkdir -p "$(dirname "$DEST")"
curl -fsSL "$URL" -o "$DEST"

# 节日名去英文括号（"中秋(Mid-autumn Festival)" -> "中秋"）；
# 补班条目统一为"补班"——它是 kde-calendar-workday-dot 红点补丁的匹配标记，勿改名
sed -i -E 's/"法定工作日[^"]*"/"补班"/g; /补班/!s/"([^"()]+)\([^"]*\)"/"\1"/g' "$DEST"

systemctl --user restart plasma-plasmashell
echo "updated: $DEST"
