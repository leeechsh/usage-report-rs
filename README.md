# usage-report（Rust）

作者：leeechsh。Linux / ext4 用户配额总览、SQLite 缓存和按 UID 的只读 TUI。

从源码构建安装（需要 Linux、Rust 工具链及 C 编译器）：

```bash
bash scripts/build-dist.sh
sudo bash install-system.sh
# 已安装的机器升级：sudo bash install-system.sh --upgrade
```

Git 仓库只包含源码、依赖锁文件、文档、测试及安装脚本。`target/`、`dist/`、
`var/` 与数据库文件不提交；构建脚本生成发布程序及 SHA256 校验清单。
清理本项目编译缓存可在项目目录运行 `cargo clean`，本地 `dist/` 的发布程序仍保留。

0.1.5 支持英文和中文，**每次启动默认英文**，不随系统 LANG 自动切换。

```bash
usage-report                       # 英文界面
usage-report --lang zh              # 直接打开中文界面
usage-report leeechsh --lang en
usage-report --plain --lang zh      # 中文文本报告
usage-report --config --lang en     # 英文配置报告
usage-report --lang zh --help       # 中文命令说明
```

在总览、目录树、进度页、配置弹窗中均可按小写 `l` 即时切换语言；界面底部显示
`l 中文` 或 `l EN`。切换保留当前选择、目录和未保存的配置输入；本次会话的语言
不会写入系统扫描配置，下一次启动仍为英文。用户名、文件名、路径和 JSON 字段
及原始数据保持不变；已知应用提示会翻译，未收录的底层诊断保留原文。
英文长状态文字使用适配列宽，80 列仍保留硬限制数值，语言切换不触发重新扫描。
预览位于 `var/ui-preview-i18n/en/`、`var/ui-preview-i18n/zh/`，可通过
`cargo run --example preview-ui -- var/ui-preview-i18n/zh zh` 重新生成中文预览。

0.1.4 将顶部三张状态卡片合并为无边框紧凑状态栏，保留容量/可用空间、用户与
关注数、索引状态。宽屏仅占一行，相比原 6 行卡片为列表释放 5 行；窄窗口自动
换行，进度页的条目数、耗时和速度也使用相同布局。预览在 `var/ui-preview-compact/`。

0.1.3 新增管理员检索配置：在 TUI 按 `c` 打开配置窗口，或通过下面的命令查看/修改。

```bash
usage-report --config                    # 所有用户可查看实际生效配置和下次执行时间
usage-report --scan-interval 1h           # 管理员：扫描结束后间隔 1 小时
usage-report --boot-delay 10m --scan-jitter 30s
usage-report --auto-scan off              # 关闭自动检索；当前扫描继续运行
usage-report --auto-scan on               # 恢复自动检索
usage-report --config --json              # 结构化配置
```

管理员为 root 和 `/etc/default/usage-report` 中 `--admin-uid` 指定的 UID，沿用现有
跨用户查询权限。已配置管理员修改设置不需要逐次 sudo；后台验证 Unix socket 对端
真实 UID。普通用户只读查看，非特权演示服务始终禁止修改系统计划。

配置窗口使用 ↑↓/Tab 选择字段，Backspace 修改时间，←→/空格切换开关，Enter 保存，
Esc 取消并返回。检索间隔支持 1 分钟至 30 天，开机延迟 0 至 1 天，随机延迟最多
1 小时且不能超过检索间隔；支持 `30m`、`3h`、`1d`、`1h30m` 等格式。
配置没有变化时不会重启 timer。实际修改会重新计算下次执行时间；开机触发时间
已经过去时，重启定时器可能立即启动一次扫描。关闭自动检索不会停止已经运行的
扫描，也不影响管理员手动 `--refresh`。

设置原子写入 `/etc/systemd/system/usage-report-scan.timer.d/50-usage-report.conf`，
随后重载并应用，读取实际生效值校验；失败时尝试恢复旧文件及 timer 状态。
升级器保留覆盖文件及 timer 的启用/运行状态，避免升级后重新开启已关闭的自动检索。
本功能不改变扫描根目录、挂载排除、配额刷新间隔或数据库格式，无需重建索引。
升级至 0.1.3 后才能使用这些接口，单独替换客户端不足以开启后台配置功能。

0.1.2 更新界面：深色面板、容量/用户/索引概览卡片、配额用量条、数字右对齐、
交替行背景及选中高亮。宽屏增加当前用户详情和配额状态分布；内容最大宽度
176 列并居中，避免超宽用户名列。窄窗口逐步精简列，低于最小尺寸提示放大。
目录树将文件名放在首列；进度页使用相同风格。快捷键和缓存查询方式沿用原行为。

已有部署更新界面：

```bash
sudo bash /home/leeechsh/Workspace/usage-report-rs/install-system.sh --upgrade
```

也可以直接运行项目中的 `dist/usage-report` 体验新客户端，无需修改系统服务。
静态预览保存在 `var/ui-preview/`；`cargo run --example preview-ui` 从运行中的只读
服务重新生成 SVG/文本预览，其中 `progress-demo` 使用明确构造的演示进度，其余
预览使用实际缓存数据。安装更新无需全盘重新索引。

```bash
usage-report                 # 终端显示配额总览，回车进入选中用户
usage-report leeechsh        # 指定用户的根分区占用树
usage-report 1004            # 指定 UID
usage-report --plain         # 原脚本风格的文本汇总，非终端输出默认如此
usage-report leeechsh --json # 结构化的首屏目录数据
usage-report --status        # 配额更新时间、已发布版本和当前扫描进度
usage-report --progress      # 交互式索引进度页，每秒刷新
usage-report --progress --plain # 单次文本进度
usage-report --watch         # 每秒追加一份文本进度，Ctrl-C 退出
usage-report --watch --json  # 每秒一行 JSON 状态
usage-report --refresh       # 管理员请求后台更新；不等待完成
```

后台服务定期刷新配额，systemd timer 约每 3 小时构建新索引。浏览时只读取数据库；
没有索引会明确提示，前台不会隐式启动昂贵的扫描。回车/右键进入目录，左键返回，
PageDown/PageUp 翻页，r 切换最新索引，Esc 回总览，q 退出。没有删除文件功能。
总览或文件树中按 p 切换索引进度页，再按 p 返回原页面。进度包含处理条目、
耗时、平均处理速度、错误/消失/跳过数、最后更新时间；管理员还能看到最近处理的目录。
扫描器每 2,000 个已插入条目或约 1 秒提交一次进度，客户端每秒读取；底层 I/O
阻塞时无法保证按秒更新，超过 10 秒未更新会提示检查服务，不直接判断任务已失败。
总文件数未知，因此不显示百分比或预计剩余时间。平均速度按最近一次已提交进度计算。
空闲时显示最近完成索引的条目数和耗时，不会为了查看进度触发新扫描。

## 数据与统计

- 用 Linux `quotactl` 的 Q_GETNEXTQUOTA 枚举内核配额，保持现有脚本的 80% 阈值、
  状态优先级、排序、软硬限制和汇总。演示模式仅能查询进程属主自己的配额。
- 0.1.1 修正根用户枚举到结尾时的 ENOENT 处理，同时兼容 ESRCH；枚举前验证
  块设备存在及 Q_GETINFO 成功，设备缺失、配额未启用和 I/O 错误仍明确报告。
  Linux quota tree 的 [find_next_id 源码](https://codebrowser.dev/linux/linux/fs/quota/quota_tree.c.html#find_next_id)
  在枚举耗尽时返回 ENOENT；旧实现只认 ESRCH，会丢弃此前累计的配额行。
- 一次扫描整个根分区，记录所有 UID；不按用户重复遍历，也不读取文件内容。
- 已知挂载点在 stat 前排除，包括同设备 bind mount，避免进入 NAS、独立 SSD、
  proc/sys/dev 等。扫描器使用 openat/O_NOFOLLOW 和目录 FD 遍历，符号链接不跟随。
- 使用 st_blocks × 512 统计实际空间，保留逻辑大小；归属目标 UID 的目录块也计入。
- 按设备与 inode 去重。硬链接首次遇到的路径计费，其他路径显示“链接引用”，
  计费空间为 0，从而目录汇总可以逐层相加。该主路径可能随两次扫描的目录枚举顺序改变。
- 其他 UID 的祖先目录保留为导航骨架，只汇总目标 UID 的后代占用。
- 数据库、WAL、SHM 和扫描锁自身排除，避免索引对自身反复计费。
- 读取错误、扫描期间消失、非 root 扫描均明确标记。活跃业务下不是文件系统快照，
  无法从路径树列出已删除但仍打开的文件、被挂载遮住的底层文件，因此不会承诺与
  内核实时配额完全相等。配额与索引分别显示更新时间。

## 存储和权限

系统数据库 `/var/lib/usage-report/index.db` 使用 SQLite WAL，必须放本地文件系统，
不要放 NFS。元数据以原始文件名字节存储，展示时转义终端控制字符。

新索引分批写入 building 版本，完成后才发布 ready；中断不会让不完整版本覆盖旧版本。
保留最近两个版本。浏览时固定 generation，旧版本被回收后会提示按 r 更新。
不要让前台保持长数据库事务；只在单次查询期间持有读事务。

DB 和锁仅服务属主可读写；客户端通过 `/run/usage-report/report.sock` 查询。
服务端使用 SO_PEERCRED 的真实 UID 校验，普通用户只能查询自己的文件树，root 和
显式 admin UID 可查询任意用户；总配额汇总维持当前所有用户可查看的行为。
没有 setuid、sudoers 新规则、任意 SQL 或远程网络 API。

## 构建与验证

```bash
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

非特权演示（显式有限范围，不是整个根分区验收）：

```bash
mkdir -p var
target/release/usage-report scan --unprivileged --root /home/leeechsh --db "$PWD/var/index.db"
target/release/usage-report serve --unprivileged --root /home/leeechsh \
  --db "$PWD/var/index.db" --socket "$PWD/var/report.sock"
# 另一终端：
target/release/usage-report leeechsh --socket "$PWD/var/report.sock"
```

## 本机安装

无需 sudo 的交互预览（仅当前用户 home 范围，退出后关闭演示服务）：

```bash
bash /home/leeechsh/Workspace/usage-report-rs/demo.sh
```

确认 dist 二进制及 SHA256 已生成后：

```bash
sudo bash /home/leeechsh/Workspace/usage-report-rs/install-system.sh
```

安装会备份现有入口脚本，保留 `usage-report-legacy`，安装 Rust 命令和两个后台任务，
启动服务并在后台进行第一次根分区扫描。安装用户 SUDO_UID 被配置为管理员，配置在
`/etc/default/usage-report` 中；该文件存在时保留现有配置。普通用户日常查询不用 sudo。
已运行的旧服务升级时应在维护窗口执行；重启查询服务会让现有客户端的一次请求失败，
可以按 r 重新加载。不会重启机器、修改配额或删除用户文件。

已有部署升级至 0.1.1：

```bash
sudo bash /home/leeechsh/Workspace/usage-report-rs/install-system.sh --upgrade
usage-report --status
usage-report --progress
```

`--upgrade` 保留现有数据库、管理员配置和 legacy 命令，备份新旧前后台二进制，
仅重启查询服务，不主动启动全盘重扫；若扫描仍在运行，会在改文件前退出并提示等待。
数据库 schema 1 自动事务升级到 schema 2，仅给 runs 小表新增进度字段，现有
nodes/usage/inodes 及 2,342 万条已完成索引无需重建。旧扫描没有目录和更新时间字段，
完整的实时进度会在下一次定时或手动扫描时出现。
回滚到 0.1.0 后其写入端不接受 schema 2，回滚前需停止新旧写入任务并另行处理
数据库兼容性；不要用旧扫描器直接覆盖新数据库。

```bash
usage-report --status
systemctl status usage-report.service usage-report-scan.service usage-report-scan.timer
journalctl -u usage-report-scan.service -n 50 --no-pager
# 管理员手动请求更新；已在扫描时不会并发启动第二份
sudo systemctl start --no-block usage-report-scan.service
```

完整扫描可能耗时，数据库会随文件数量增长。默认 scan 服务采用低 CPU/I/O 优先级、
2 GiB 内存上限和单扫描锁。实际全根扫描速度、峰值空间和服务资源限制需部署后测量。
同名目录的失败扫描不会替代已发布索引；后续扫描会标记旧 building 为中断。

## 回退

安装器输出并保存旧命令到 `/var/backups/usage-report/<时间>/usage-report`。
需要回退时先停止新扫描和查询服务，然后从该确切备份恢复 `/usr/local/bin/usage-report`。
数据库保留用于调查，不应在服务运行时单独复制主 DB 忽略 WAL；备份应使用 SQLite
备份 API 或在停止写入、完成 checkpoint 后操作。原 `/usr/local/sbin/quota_report` 不会被改写。
