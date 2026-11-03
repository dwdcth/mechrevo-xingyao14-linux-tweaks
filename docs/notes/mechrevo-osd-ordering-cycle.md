# mechrevo-osd 开机排序环——Fn 键全失效但服务"没死"(2026-11-03)

## 症状

GPU 事故重启(见 [smu-mailbox-brick-amdgpu.md](smu-mailbox-brick-amdgpu.md))之后,
Fn+X / Fn+F8 等全部失效。`systemctl status mechrevo-osd` 显示 `inactive (dead)`
但 `enabled`——没有 crash、没有 failed、journal 里该单元几乎无记录。

## 根因:三点排序环,systemd 静默删任务

```
multi-user.target ──Wants──> mechrevo-osd.service     ([Install] WantedBy=multi-user)
mechrevo-osd     ──After──> graphical.target          (单元体 After=)
graphical.target ──After──> multi-user.target         (systemd 默认)
```

开机事务里成环,systemd 的处理是**删除环上一个任务**——选中了 osd 的启动任务,
只在 journal 留一条:

```
multi-user.target: Found ordering cycle: mechrevo-osd.service/start after
graphical.target/start after multi-user.target/start - after mechrevo-osd.service
multi-user.target: Job mechrevo-osd.service/start deleted to break ordering cycle
```

不是错误、不置 failed、服务无声消失。**排查口诀:Fn 键失灵先
`journalctl -b | grep "ordering cycle"`。**

## 为什么改完 unit 当次没事、下次重启才炸

- 旧部署的 /etc unit 没有这个组合,一直正常;
- 2026-11-03 09:33 重部署仓库版 unit(引入 `After=graphical.target` 的完整版)+ `daemon-reload`
  ——**daemon-reload 只替换定义,不重演启动顺序**,当时服务还在跑,一切看似正常;
- 12:15 关机时环第一次在事务里现身(删除的是 multi-user 的 stop 任务,无感),
  12:18 开机删除 osd 的 start 任务 → Fn 全灭。

教训:**改 unit 的 After/Before/WantedBy 后,当前会话的"正常"不能证明没问题,
必须重启验证(或 `systemctl start` 一个会拉全链路的事务)。**

## 修复

`[Install] WantedBy=graphical.target`(OSD 依赖用户 DBus 会话,本就该挂在 graphical 下,
After=graphical 也就从"成环边"变成正常顺序边)。

升级旧部署时必须:

```bash
sudo systemctl disable mechrevo-osd   # 摘掉 multi-user.target.wants 旧符号链接!
sudo systemctl daemon-reload
sudo systemctl enable --now mechrevo-osd   # 重建到 graphical.target.wants
```

不先 disable 的话旧链接残留,环依旧。

## 附:同日该服务的另一种死法

REISUB/ssh 终止对 systemd 是"干净退出",`Restart=on-failure` 不拉回——已改
`Restart=always`。两种死法症状相同(Fn 失灵),journal 特征不同:
干净退出有 `Deactivated successfully` + Result=success;排序环连启动记录都没有。
