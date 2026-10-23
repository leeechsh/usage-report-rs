# usage-report

**在一个终端中查看 Linux 用户配额与磁盘占用。**

查看谁占用了根分区、配额还剩多少，并像使用 ncdu 一样浏览某个用户的文件，
即使这些文件分散在 home 目录之外。后台服务统一构建 SQLite 索引，
打开报告时无需重新遍历文件系统。

[快速开始](#快速开始) · [使用方式](#使用方式) · [检索配置](#检索配置) · [验证记录](VALIDATION.md) · [English](README.md) / **简体中文**

## 适合谁使用？

usage-report 面向共享 Linux 服务器的管理员，以及希望了解自身空间占用的用户。
主要部署目标为已启用用户配额的本地 ext4 根分区，使用 systemd 管理后台服务。

- **配额总览。** 按占用排序，显示已用空间、软硬限制、剩余额度、用量条与彩色状态提醒。
- **按 UID 浏览。** 查看某个用户在整个根分区的占用树，包括其他用户目录下的文件。
- **缓存查询。** 后台一次扫描索引所有 UID，新版本构建期间仍可浏览已完成的索引。
- **检索进度。** 查看条目数、耗时、平均速度、错误和跳过数量，无需发起新扫描。
- **管理员配置。** 通过命令行或交互界面调整自动检索计划。
- **中英文界面。** 默认英文，按 `l` 即时切换中文。

文件浏览为只读操作，不删除文件、不修改用户配额。管理员可以显式请求扫描及修改检索计划。

## 快速开始

需要 Linux、支持 edition 2024 的 Rust 工具链，以及用于编译内置 SQLite 的 C 编译器。
系统安装还需要 systemd。请事先启用用户配额；安装器不会配置配额，也不会修改 `/etc/fstab`。

```bash
git clone https://github.com/leeechsh/usage-report-rs.git
cd usage-report-rs
bash scripts/build-dist.sh
sudo bash install-system.sh
usage-report
```

克隆需要仓库访问权限。构建脚本生成 `dist/usage-report` 和 SHA256 校验清单，
安装器会验证校验值。

安装后启动查询服务，并在后台进行首次 `/` 扫描。首次扫描结束后才能浏览目录树，
可用 `usage-report --progress` 查看进度。日常查询不需要 `sudo`。

不安装系统服务的体验方式：完成构建后运行以下命令。

```bash
bash demo.sh
```

演示模式以普通用户权限索引自己的 home 目录，再次启动时复用本地缓存，退出时停止
演示查询服务。它不是全根分区扫描，也不能修改系统检索计划。

## 使用方式

```bash
usage-report                        # 交互式配额总览
usage-report alice                  # 按用户名查看占用树
usage-report 1004                   # 按 UID 查看占用树
usage-report --plain                # 纯文本配额报告
usage-report alice --json           # 目录树首屏数据，JSON 格式
usage-report --status               # 配额更新、已完成索引和当前扫描状态
usage-report --progress             # 交互式检索进度
usage-report --progress --plain     # 单次文本进度
usage-report --watch                # 每秒追加一份进度
usage-report --watch --json         # 每秒一条 JSON 状态
usage-report --refresh              # 管理员：请求后台异步扫描
```

非终端输出默认使用纯文本。请求刷新后不会等待扫描完成，也不会并发启动第二份扫描。

### 快捷键

| 按键 | 操作 |
| --- | --- |
| `↑` / `↓`、`j` / `k` | 选择行 |
| `Enter` / `→` | 打开用户占用树或进入目录 |
| `←` / `Backspace` | 返回上级目录 |
| `PgUp` / `PgDn` | 切换目录查询页，每页 200 项 |
| `r` | 载入最新已完成索引，不触发扫描 |
| `p` | 打开进度页，再按一次返回 |
| `c` | 打开检索配置 |
| `l` | 切换中英文 |
| `Esc` | 返回总览或关闭弹窗 |
| `q` / `Ctrl-C` | 退出；配置弹窗打开时，`q` 先关闭弹窗 |

界面随终端宽度调整，使用精简状态栏、数字右对齐、用量条、交替行背景和选中高亮。

### 界面语言

```bash
usage-report --lang zh              # 以中文启动
usage-report alice --lang en        # 以英文启动
usage-report --plain --lang zh      # 中文文本报告
usage-report --lang zh --help       # 中文命令帮助
```

每次启动默认英文，不随 `LANG` 改变。按 `l` 切换时保留当前选择、所在目录和未保存的
配置输入。语言选择不持久保存。用户名、文件名、路径及 JSON 字段和数据保持原样；
未收录的底层诊断保留原文。

## 检索配置

内核配额默认每 **30 秒** 刷新。全文件系统索引由独立的 systemd timer 触发，随项目分发的默认值为：

| 设置 | 默认值 |
| --- | --- |
| 开机后延迟 | 5 分钟 |
| 上次扫描结束后的间隔 | 3 小时 |
| 随机延迟 | 最多 1 分钟 |
| 定时器精度窗口 | 1 分钟 |

这些是调度窗口，不代表精确的执行时刻。查看本机实际生效配置及下次触发时间：

```bash
usage-report --config
usage-report --config --json
```

root 和已配置的管理员可直接修改，不需要每次额外输入 `sudo`：

```bash
usage-report --scan-interval 1h
usage-report --boot-delay 10m --scan-jitter 30s
usage-report --auto-scan off
usage-report --auto-scan on
```

配置弹窗（`c`）使用 `↑` / `↓` / `Tab` 选择字段，`Backspace` 编辑时长，
`←` / `→` / `Space` 切换开关，`Enter` 保存，`Esc` 取消。
普通用户可查看配置，但不能保存修改。

时长支持 `30m`、`3h`、`1d`、`1h30m` 等格式。扫描间隔为 1 分钟至 30 天，
开机延迟为 0 至 1 天，随机延迟最多 1 小时且不能超过扫描间隔。
配置原子写入 `/etc/systemd/system/usage-report-scan.timer.d/50-usage-report.conf`，
应用后读取 systemd 实际配置进行校验；失败时尝试恢复原设置。

保存未改变的配置不会重启定时器。修改后会重新计算下次执行时间；如果开机延迟已过，
可能立即触发扫描。关闭自动检索不会停止正在运行的扫描，也不影响手动 `--refresh`。

## 索引如何工作

后台以 root 权限一次遍历文件系统，索引所有 UID，只读取元数据，不读取文件内容。
进入已知挂载点前就将其排除，包括 NFS、独立磁盘、虚拟文件系统和同设备 bind mount。
不跟随符号链接。

实际占用使用 `st_blocks × 512` 统计，并单独保留逻辑大小。属于目标 UID 的目录块也计入。
硬链接按设备和 inode 去重：首次遇到的路径计费，其余路径显示为链接引用。
首次计费的路径可能随扫描变化。其他 UID 拥有的祖先目录保留为导航路径，
不将它们自身的空间计入所选用户。

新索引完成后才发布，保留最近两个已完成版本。中断的扫描不会覆盖原有可用索引。
浏览期间固定使用同一版本，按 `r` 后更新。

进度每插入 2,000 项或约 1 秒提交一次；底层 I/O 阻塞可能延迟更新。
由于总条目数未知，不显示完成百分比或剩余时间。当前扫描路径仅管理员可见，
普通用户只能查看汇总进度。

**索引是动态遍历结果，不是文件系统快照。** 已删除但仍打开的文件、被挂载遮住的文件、
扫描期间发生的修改和读取错误，都可能导致缓存统计与内核实时配额不同。
界面分别显示配额和索引的更新时间。

## 权限与存储

| 组件 | 默认位置 |
| --- | --- |
| 客户端 | `/usr/local/bin/usage-report` |
| 服务程序 | `/usr/local/libexec/usage-report` |
| SQLite 索引 | `/var/lib/usage-report/index.db` |
| Unix socket | `/run/usage-report/report.sock` |
| 管理员配置 | `/etc/default/usage-report` |

SQLite WAL 数据库仅服务属主可访问，必须放在本地文件系统；程序拒绝将数据库放在 NFS 上。
客户端通过 Unix socket 查询，服务端使用 `SO_PEERCRED` 验证真实 UID。
普通用户只能浏览自身 UID 的占用树，root 和已配置的管理员可浏览任意 UID。
配额总览对所有用户开放。

首次安装将执行 `sudo` 的用户 UID 设置为管理员，已有配置会被保留。
可在 `/etc/default/usage-report` 的 `USAGE_REPORT_ADMIN_ARGS` 中重复添加
`--admin-uid` 参数以指定多个管理员；编辑后重启 `usage-report.service`。
不安装 setuid 程序、sudoers 规则或远程网络 API。

## 升级与排查

```bash
bash scripts/build-dist.sh
sudo bash install-system.sh --upgrade
usage-report --status
```

升级前请等待当前扫描结束；扫描期间安装器会在修改文件之前拒绝升级。
升级保留数据库、管理员配置、定时器覆盖文件，以及定时器的启用和运行状态。
仅重启查询服务，不主动发起全盘重建。

现有程序备份至 `/var/backups/usage-report/<时间戳>/`。
已有入口命令保留为 `usage-report-legacy`，原 `/usr/local/sbin/quota_report` 不会被覆盖。
恢复旧程序时也需要确认数据库版本兼容性，详见[验证与升级记录](VALIDATION.md)。

```bash
systemctl status usage-report.service usage-report-scan.service usage-report-scan.timer
journalctl -u usage-report-scan.service -n 50 --no-pager
```

大型文件系统扫描需要时间，数据库空间随元数据数量增长。
随项目分发的扫描服务使用较低的 CPU/I/O 优先级和 2 GiB 内存上限，
实际资源需求应根据工作负载测量。备份数据库请使用 SQLite 备份 API，
或停止写入并完成 checkpoint；WAL 写入期间仅复制主数据库文件是不完整的。

## 开发

```bash
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
```

| 路径 | 用途 |
| --- | --- |
| `src/scan.rs`、`src/db.rs`、`src/model.rs` | 遍历、SQLite 存储和数据模型 |
| `src/quota.rs`、`src/service.rs` | 内核配额查询和身份验证服务 |
| `src/tui.rs`、`src/ui.rs` | 交互逻辑和终端渲染 |
| `src/i18n.rs`、`src/text.rs` | 多语言和文本报告 |
| `src/schedule.rs`、`systemd/` | 检索调度和服务单元 |
| `tests/`、`examples/` | 集成测试和界面预览生成器 |
| `scripts/build-dist.sh`、`install-system.sh` | 构建和系统安装 |
| [VALIDATION.md](VALIDATION.md) | 各版本验证与运维记录 |

构建输出（`target/`、`dist/`）、本地预览（`var/`）和数据库文件不提交到 Git。
运行 `cargo clean` 可清除编译产物，本地 `dist/` 发布程序仍会保留。

## 许可证

[MIT](LICENSE) · 作者：[leeechsh](https://github.com/leeechsh)。
