# Zenbook Duo 开机带键盘导致 Greeter 阶段副屏异常排查纪要

## 1. 问题定义

现象（用户稳定复现）：

- 仅当“带键盘开机”时，登录界面（GDM Greeter）阶段会出现副屏变黑/卡顿。
- 一旦该阶段触发异常，进入桌面后副屏持续异常（开关失效、卡顿、显示链路不稳定）。
- 不带键盘开机时，系统整体行为正常。

用户结论（多次实测）：

- 该问题不是偶发，属于“有条件必现”：
  - 条件：开机时带键盘
  - 结果：100% 触发异常

## 2. 本轮取证方法

新增独立取证工具目录：

- `debug/greeter-forensics/`

工具包括：

- `collector.sh`：系统级持续采集（greeter 前启动）
- `analyzer.py`：自动生成归因报告
- `greeter-forensics.service` + 安装/卸载脚本

采集源覆盖：

- Kernel DRM 链路：`kernel-drm.log`
- 内核输入/驱动链路：`kernel-input-power.log`
- Greeter/Mutter：`gdm-mutter.log`
- GNOME 电源/会话相关：`gnome-power.log`
- udev 设备事件：`udev.log`
- 时序主线：`timeline.tsv`
- DRM 快照：`state-snapshots/`

## 3. 关键证据文件

最近两份对比报告路径：

- `/home/arnold/Projects/zenbook-duo-linux-main/debug/greeter-forensics/report-20260408T182132Z-d5febc57-8037-41c0-a629-c7e650b7427c-unspecified.md`
- `/home/arnold/Projects/zenbook-duo-linux-main/debug/greeter-forensics/report-20260408T181903Z-5aa3b022-939c-4939-a9c5-7607e2ddedac-unspecified.md`

对应原始 run 目录：

- `/var/log/zenbook-duo-forensics/20260408T182132Z-d5febc57-8037-41c0-a629-c7e650b7427c-unspecified`
- `/var/log/zenbook-duo-forensics/20260408T181903Z-5aa3b022-939c-4939-a9c5-7607e2ddedac-unspecified`

## 4. 已确认事实

### 4.1 不是项目服务在 Greeter 阶段主动关屏

在“强禁用/隔离”验证后（`zenbook-duo-rust-daemon/lifecycle/boot-trace` 不再启动），
仍可复现异常。

本轮 run 的 `zenbook-units.log` 中未出现 `Started zenbook-duo-rust-daemon.service` 等启动痕迹，
仅有后续用户态 `zenbook-duo-control` 日志。

结论：

- 主服务不是 Greeter 阶段首发触发源。

### 4.2 副屏“黑”不等于被软件 disable

在两个 run 中：

- `timeline.tsv` 与 `state-snapshots/` 显示 `card0-eDP-2.status=connected`
- `card0-eDP-2.enabled=enabled`

没有看到 `enabled -> disabled` 的证据。

结论：

- 当前更像“链路/驱动/背光管线异常”，而不是 display layout 把副屏显式关闭。

### 4.3 键盘事件与 Greeter 显示初始化窗口重叠

在带键盘场景日志里可见：

- `ASUS Zenbook Duo Keyboard` / `Primax ... ASUS Zenbook Duo Keyboard` 枚举事件
- 与 `gdm-greeter` + `gnome-shell (mutter)` 的 KMS 初始化处于同一时间窗口（开机后数秒）

结论：

- “键盘接入状态”与异常触发存在稳定相关性。

### 4.4 内核 DRM 异常在问题场景出现

问题 run 中出现：

- `xe ... [drm] *ERROR* Failed to read DPCD register 0x60`
- 连续 `Timed out waiting for PSR Idle for re-enable`

结论：

- 当前最强技术线索指向 `xe/eDP/PSR` 显示驱动路径。

## 5. 当前判断（工作结论）

综合用户“带键盘 100% 触发”与日志证据，当前判断是：

1. 触发条件：开机时键盘接入状态（USB/BT/HID 设备链路）
2. 触发阶段：Greeter 初始化显示栈（GDM + Mutter KMS）
3. 受损路径：内核显示驱动（`xe/eDP/PSR`）进入错误状态
4. 后果：进入桌面后副屏链路已被污染，表现为卡顿/黑屏/控制异常

该结论优先级高于“项目服务逻辑错误触发关屏”的假设。

## 6. 已排除/弱化的假设

- “Rust daemon 8 秒延迟误触发关屏”：证据不足，且服务禁用后仍可触发。
- “副屏被显式 SetDockMode 关闭”：当前采集中无对应先发命令证据。
- “Lid close 直接触发”：本轮 `lid_state` 为 `open`，未看到直接 lid close 证据。

## 7. 数据局限说明

- 历史报告文件名中的 `scenario=unspecified`，是因为当时未填写场景标签；
  但用户已明确通过人工流程区分“带键盘/不带键盘”并稳定复现。
- 报告中的自动归因字段目前仍显示 `unknown`，主要因为黑屏表现是“显示异常”而非
  `eDP-2 enabled` 的布尔切换；这并不否定驱动层问题。

## 8. 下一步建议（按因果验证优先）

建议做“单变量 A/B”验证，优先确认 PSR 路径：

1. 固定“带键盘开机”条件。
2. 只改一个变量：临时关闭 PSR 参数（不改项目代码）。
3. 复测是否仍 100% 触发。

若关闭 PSR 后问题显著缓解，则可基本锁定 `xe/eDP/PSR` 为主因；
再决定是否做长期方案（内核参数、驱动版本策略、Greeter 阶段规避策略）。

## 9. 文档生成时间

- 生成时间（本地时区）：2026-04-09
- 依据：本地取证日志与上述两份 report 及对应 run 原始日志
