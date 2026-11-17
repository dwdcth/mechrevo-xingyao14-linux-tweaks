/*
    SPDX-FileCopyrightText: 2011 Viranch Mehta <viranch.mehta@gmail.com>
    SPDX-FileCopyrightText: 2013 Kai Uwe Broulik <kde@privat.broulik.de>
    SPDX-License-Identifier: LGPL-2.0-or-later
*/

import QtQuick

import org.kde.kirigami as Kirigami

Item {
    id: root

    property bool hasBattery
    property int percent
    property bool pluggedIn
    property string batteryType
    property bool isSomehowFullyCharged: false
    property bool active: false
    property string powerProfileIconName: ""
    property bool preferSymbolic: false

    // ==== 星耀14 局部补丁:pluggedIn 防抖(托盘闪电随机闪烁) ====
    // 本机 EC 在充电阈值点(T1=80%)每几秒横跳电池状态位(实测 40s 内
    // Charging/Discharging/Not charging = 13/25/2 次),UPower 30s 轮询跟着抖
    // → 本组件 pluggedIn 抖 → "-charging" 图标随机闪烁(电量实际纹丝不动)。
    // 规则:新状态须持续 plugDebounceMs 才采纳;首个状态立即采纳,不拖会话启动。
    // 证据链与排障背景见仓库 docs/notes/battery_analysis.md
    property int plugDebounceMs: 120000
    property bool stablePluggedIn: false
    property bool plugSynced: false
    Timer {
        id: plugDebounce
        interval: root.plugDebounceMs
        onTriggered: root.stablePluggedIn = root.pluggedIn
    }
    onPluggedInChanged: {
        if (!plugSynced) {
            stablePluggedIn = pluggedIn;
            plugSynced = true;
        } else {
            plugDebounce.restart();
        }
    }
    Component.onCompleted: {
        if (!plugSynced && pluggedIn) {
            stablePluggedIn = true;
            plugSynced = true;
        }
    }
    // ==== 补丁结束 ====

    // Icon for current charge level, charging status, and optionally power
    // profile indication (for batteries that support it by setting
    // "powerProfileIconName" to something other than an empty string).
    Kirigami.Icon {
        anchors.fill: parent
        source: (root.hasBattery ? fillElement(root.percent) : "battery-missing") + (root.preferSymbolic ? "-symbolic" : "")
        visible: !otherBatteriesIcon.visible
        active: root.active

        function fillElement(p: int): string {
            // Don't show the battery icon when fully charged and plugged in;
            // it adds no value here. Only show the power profile icon
            if (root.isSomehowFullyCharged && root.powerProfileIconName) {
                return "battery-profile-" + root.powerProfileIconName
            }

            let name
            if (p >= 95) {
                name = "battery-100";
            } else if (p >= 85) {
                name = "battery-090";
            } else if (p >= 75) {
                name = "battery-080";
            } else if (p >= 65) {
                name = "battery-070";
            } else if (p >= 55) {
                name = "battery-060";
            } else if (p >= 45) {
                name = "battery-050";
            } else if (p >= 35) {
                name = "battery-040";
            } else if (p >= 25) {
                name = "battery-030";
            } else if (p >= 15) {
                name = "battery-020";
            } else if (p > 5) {
                name = "battery-010";
            } else {
                name = "battery-000";
            }

            if (root.stablePluggedIn) {
                name += "-charging";
            }

            if (root.powerProfileIconName.length > 0) {
                name += "-profile-" + root.powerProfileIconName
            }

            return name;
        }
    }

    // Generic icon for other types of batteries
    Kirigami.Icon {
        id: otherBatteriesIcon
        anchors.fill: parent
        source: elementForType(root.batteryType) + (root.preferSymbolic ? "-symbolic" : "")
        visible: elementForType(root.batteryType) !== ""
        active: root.active

        function elementForType(t: string): string {
            switch(t) {
                case "Mouse":
                    return "input-mouse-battery";
                case "Keyboard":
                    return "input-keyboard-battery";
                case "Pda":
                    return "phone-battery";
                case "Phone":
                    return "phone-battery";
                case "Ups":
                    return "battery-ups";
                case "GamingInput":
                    return "input-gaming-battery";
                case "Bluetooth":
                    return "preferences-system-bluetooth-battery";
                case "Headphone":
                    return "audio-headphones-battery";
                case "Headset":
                    return "audio-headset-battery";
                default:
                    return "";
            }
        }
    }
}
