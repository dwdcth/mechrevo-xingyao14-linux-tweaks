# KDE 锁屏指纹直触解锁（免按键）

**效果**：锁屏后（含熄屏）任意时刻**轻触电源键直接解锁**，不用先按别的键唤出密码框。指纹传感器与电源键二合一的机型专用。

**原理**：Plasma 锁屏 QML（`LockScreenUi.qml`，属 **plasma-desktop** 包）默认 `uiVisible=false`，认证会话（含 fprintd 监听）要等第一次输入事件才启动；补丁让锁屏加载即监听 + 心跳全程续命。两个补丁缺一不可（只打第一个，会话数十秒后静默死亡）。

> 来源启发：[nikhcevov.me 的 Plasma 指纹解锁方案](https://nikhcevov.me/posts/kde-plasma-fingerprint-unlock-immediately)，在本机迭代四版定稿（v4，演化史见仓库 README 链接的文档）。

## 前提

- KDE Plasma 6.8（plasma-desktop 6.8.0 验证；上游 QML 结构变化时脚本会**报错退出而非静默失效**，届时需更新匹配行）
- 指纹已录入（`fprintd-enroll`）

## 安装

```bash
sudo install -Dm755 kde-lock-immediate /usr/local/bin/kde-lock-immediate
sudo install -Dm755 kde-lock-raise /usr/local/bin/kde-lock-raise
sudo install -Dm644 kde-lock-immediate.hook /etc/pacman.d/hooks/kde-lock-immediate.hook
sudo /usr/local/bin/kde-lock-immediate   # 立即打补丁（幂等，可重复执行）
```

配套 systemd 件（恢复后抬密码框 + 挂起前释放指纹器）在 [../systemd/INSTALL.md](../systemd/INSTALL.md)。

## 验证

1. `sudo systemctl suspend` 或直接锁屏（Meta+L）
2. 熄屏后放置 2 分钟以上，**轻触**（勿按重，按重触发电源键事件）电源键 → 应直接进桌面
3. 密码路径正常：按任意键 → 输密码 → 解锁，无循环

## 卸载 / 排查

```bash
sudo rm /etc/pacman.d/hooks/kde-lock-immediate.hook /usr/local/bin/kde-lock-immediate
# 整包还原 QML（勿用 bsdtar 抽单文件+重定向，会清空文件）：
sudo pacman -S plasma-desktop    # 或从 /var/cache/pacman/pkg/ 用 pacman -U 旧版本
```

若锁屏出现「输密码又弹回」循环：先拆本补丁排查——注意 Plasma 6.8 原厂锁屏在特定场景（KDE bug 375536/490924 家族）也有同款循环，拆补丁前先看 journalctl 确认因果。
