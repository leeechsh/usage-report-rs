#!/usr/bin/env bash
set -euo pipefail
PROJECT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
UPGRADE=false
case "${1:-}" in
    --upgrade) UPGRADE=true ;;
    '') ;;
    *) printf '用法：sudo bash %s [--upgrade]\n' "${BASH_SOURCE[0]}" >&2; exit 1 ;;
esac
if [ "$(id -u)" -ne 0 ]; then
    printf '请执行：sudo bash %s\n' "${BASH_SOURCE[0]}" >&2
    exit 1
fi
test -x "$PROJECT_DIR/dist/usage-report"
(cd "$PROJECT_DIR/dist" && sha256sum -c SHA256SUMS)
test -d /run/systemd/system
if "$UPGRADE"; then
    test -x /usr/local/libexec/usage-report
    if systemctl is-active --quiet usage-report-scan.service; then
        printf '扫描仍在进行，请等完成后升级；本次未修改系统文件。\n' >&2
        exit 1
    fi
fi
TIMER_WAS_ENABLED=true
TIMER_WAS_ACTIVE=true
if "$UPGRADE"; then
    if ! systemctl is-enabled --quiet usage-report-scan.timer; then TIMER_WAS_ENABLED=false; fi
    if ! systemctl is-active --quiet usage-report-scan.timer; then TIMER_WAS_ACTIVE=false; fi
fi
ADMIN_UID="${SUDO_UID:-0}"
case "$ADMIN_UID" in ''|*[!0-9]*) printf '无效的管理员 UID\n' >&2; exit 1;; esac
BACKUP_DIR="/var/backups/usage-report/$(date +%Y%m%d-%H%M%S)"
install -d -m 700 /var/lib/usage-report "$BACKUP_DIR"
install -d -m 755 /usr/local/libexec
if [ -e /usr/local/libexec/usage-report ]; then
    cp -a /usr/local/libexec/usage-report "$BACKUP_DIR/usage-report-backend"
fi
if [ -e /usr/local/bin/usage-report ]; then
    cp -a /usr/local/bin/usage-report "$BACKUP_DIR/usage-report"
    if [ ! -e /usr/local/bin/usage-report-legacy ]; then
        cp -a /usr/local/bin/usage-report /usr/local/bin/usage-report-legacy
    fi
fi
if [ -e /usr/local/sbin/quota_report ]; then
    cp -a /usr/local/sbin/quota_report "$BACKUP_DIR/quota_report"
fi
for config in /etc/default/usage-report /etc/systemd/system/usage-report.service /etc/systemd/system/usage-report-scan.service /etc/systemd/system/usage-report-scan.timer; do
    if [ -e "$config" ]; then cp -a "$config" "$BACKUP_DIR/$(basename "$config").old"; fi
done
install -m 755 "$PROJECT_DIR/dist/usage-report" /usr/local/libexec/usage-report.new
mv -f /usr/local/libexec/usage-report.new /usr/local/libexec/usage-report
for unit in usage-report.service usage-report-scan.service usage-report-scan.timer; do
    install -m 644 "$PROJECT_DIR/systemd/$unit" "/etc/systemd/system/$unit"
done
systemd-analyze verify --generators=false --man=false \
    /etc/systemd/system/usage-report.service \
    /etc/systemd/system/usage-report-scan.service \
    /etc/systemd/system/usage-report-scan.timer
if [ ! -e /etc/default/usage-report ]; then
    printf 'USAGE_REPORT_ADMIN_ARGS="--admin-uid %s"\n' "$ADMIN_UID" > /etc/default/usage-report
    chmod 600 /etc/default/usage-report
fi
systemctl daemon-reload
systemctl enable usage-report.service
if "$TIMER_WAS_ENABLED"; then systemctl enable usage-report-scan.timer; fi
systemctl restart usage-report.service
# Only replace the public command after the backend answers successfully.
for attempt in 1 2 3 4 5; do
    if /usr/local/libexec/usage-report --status; then break; fi
    if [ "$attempt" -eq 5 ]; then printf '服务未就绪，原入口未替换。\n' >&2; exit 1; fi
    sleep 1
done
install -m 755 "$PROJECT_DIR/dist/usage-report" /usr/local/bin/usage-report.new
mv -f /usr/local/bin/usage-report.new /usr/local/bin/usage-report
if "$TIMER_WAS_ACTIVE"; then systemctl start usage-report-scan.timer; fi
if ! "$UPGRADE"; then systemctl start --no-block usage-report-scan.service; fi
printf '\n安装完成。旧命令备份：%s\n' "$BACKUP_DIR"
if "$UPGRADE"; then
    printf '升级完成，保留现有索引；未主动启动全盘扫描。\n'
else
    printf '首次全盘索引在后台构建。\n'
fi
printf 'usage-report --progress 查看进度页，--watch 每秒输出进度。\n'
printf '管理员 UID：%s；普通用户只能浏览自己的文件树。\n' "$ADMIN_UID"
printf '原汇总脚本仍可通过 usage-report-legacy 调用。\n'
