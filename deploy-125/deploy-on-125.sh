#!/usr/bin/env bash
#
# OpenObserve 太湖 SAML 版 —— 在 43.128.101.125 上部署（生产机，opensource 构建）
#
# 本脚本只做三件事，全部可回滚，【绝不触碰 ZO_DATA_DIR 数据目录和任何日志】：
#   1. 用新二进制替换旧二进制（旧的先备份，失败自动回滚）
#   2. 写入 systemd override（SAML + cookie 环境变量）
#   3. 重启服务并做健康检查
#
# 用法（在 43.128.101.125 上，二进制已放到 /tmp/openobserve-saml）：
#   sudo bash deploy-on-125.sh
#
# 回滚：sudo bash deploy-on-125.sh --rollback
set -euo pipefail

# ------------------------------------------------------------------ 配置
# 优先用脚本同目录下的二进制(打包随行)，否则回退到 /tmp/openobserve-saml
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "${SCRIPT_DIR}/openobserve-saml" ]]; then
  NEW_BIN="${SCRIPT_DIR}/openobserve-saml"
else
  NEW_BIN="/tmp/openobserve-saml"
fi
EXPECT_SHA="aba143ae2dd75d0b6ccc8d9f67d28a865dd03783040e72e6bcfffbb4cff09a68"
SERVICE="openobserve"                                 # systemd 服务名（若不同，改这里）
SP_HOST="https://43.128.101.125"                      # 本机对外 https 入口（经 nginx 443）
IDP_CERT="MIIDGTCCAgGgAwIBAgIJAJdghuWVcni4MA0GCSqGSIb3DQEBCwUAMCMxITAfBgNVBAMMGG9kYy10YWkuaXQudGVuY2VudGNzLmNvbTAeFw0yMzA3MjQwNzE3NTJaFw0zMzA3MjEwNzE3NTJaMCMxITAfBgNVBAMMGG9kYy10YWkuaXQudGVuY2VudGNzLmNvbTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAK/hrZXw51snX0zqAi47YoTapYcIUb4TGyP5z0AXNB2+kSO6Gk9CVLOCqRds3vCIQ301NROtj2gEPeHnUrLo1X43+al4ikny3ZH/0PchOCW+nyQ+AwKXmVcUFEyK/Q8pAHOF/6VL5ru9Wp1nx+LnKBPKrJNdRqbNG6UzfzIpdICkUzj/qiHYpz2dxThgIybokUWMR09fbNn0ryjcT46GUmkG8X5aUsrFcI93FTR8gbsZ+PEf7FhYlobWxEGlmxy99SwT2pERULPCxYAPaNqJU/qtFfSa2Z68XS8aZSHYM+keJRUGt8XOnTzmyRr/ruuK3BnmyMwWHSUygs2wn0l6gCMCAwEAAaNQME4wHQYDVR0OBBYEFDp6FRdYKi5J8vDWqBJx/k9iNL/OMB8GA1UdIwQYMBaAFDp6FRdYKi5J8vDWqBJx/k9iNL/OMAwGA1UdEwQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEBAAJziVR7DyuyWIHcz4bDahz+Kfp6NnNIeeNxC/V3SivS6BWPU5689kzvo5ibstmfmTFPm41IQTWVHIRFVrWueaqKRle+sjd3WyWE779S0NVzFt293ETXSEQkAHBqj06fsFA0IiIIExfD9Gf3o4EVAzUDQ1xgW4qOJPcOFGwXVWX9Zi3lrzTwRmKB+ocqsb375c+X3zccK65jRXOafbhOwctrYRRT65rLwZ3qgcjinezVmwmdMRfJPJgKAzbwuTnLqJ17kSIQmV3NBZ9LwNtsYvxutbC5UoooQuPFS+Zm3mM21Q7tAlwQXy0HFbmihbG4pe+ZMBUUT0rdDueTGVftVN8="

BAK_DIR="/opt/o2-saml-backup"
OVERRIDE_DIR="/etc/systemd/system/${SERVICE}.service.d"
OVERRIDE_FILE="${OVERRIDE_DIR}/saml.conf"

log(){ echo -e "\033[1;32m[deploy]\033[0m $*"; }
err(){ echo -e "\033[1;31m[error]\033[0m $*" >&2; }

if [[ "${1:-}" == "--rollback" ]]; then
  log "回滚模式"
  LATEST_BAK=$(ls -1t ${BAK_DIR}/openobserve.bak.* 2>/dev/null | head -1 || true)
  [[ -z "$LATEST_BAK" ]] && { err "找不到备份，无法回滚"; exit 1; }
  BIN_PATH=$(cat ${BAK_DIR}/bin_path.txt)
  systemctl stop "$SERVICE" || true
  install -m 0755 "$LATEST_BAK" "$BIN_PATH"
  rm -f "$OVERRIDE_FILE"
  systemctl daemon-reload
  systemctl start "$SERVICE"
  log "已回滚二进制 ($LATEST_BAK -> $BIN_PATH) 并移除 SAML override"
  exit 0
fi

[[ $EUID -ne 0 ]] && { err "请用 sudo 运行"; exit 1; }

# ---------------------------------------------------------- 0. 前置校验
log "检查新二进制: $NEW_BIN"
[[ -f "$NEW_BIN" ]] || { err "未找到 $NEW_BIN，请先把二进制搬到这里"; exit 1; }
GOT_SHA=$(sha256sum "$NEW_BIN" | awk '{print $1}')
if [[ "$GOT_SHA" != "$EXPECT_SHA" ]]; then
  err "sha256 不匹配！传输可能损坏。"
  err "  期望: $EXPECT_SHA"
  err "  实际: $GOT_SHA"
  exit 1
fi
log "sha256 校验通过"

log "检查 glibc (需 >= 2.35)"
GLIBC=$(ldd --version | head -1 | grep -oE '[0-9]+\.[0-9]+' | head -1)
log "  glibc = $GLIBC"
awk "BEGIN{exit !($GLIBC >= 2.35)}" || { err "glibc < 2.35，二进制无法运行，请改用源码编译或容器方案"; exit 1; }

log "检查 build_type (需 opensource)"
BT=$(curl -s http://127.0.0.1:5080/config | grep -oE '"build_type":"[^"]*"' || true)
log "  $BT"
if ! echo "$BT" | grep -q opensource; then
  err "当前实例不是 opensource（$BT），直接替换会丢失企业功能，已中止。"
  exit 1
fi

# ---------------------------------------------------- 1. 定位 + 备份旧二进制
PID=$(pgrep -f "${SERVICE}" | head -1 || true)
[[ -z "$PID" ]] && { err "未找到运行中的 ${SERVICE} 进程，请确认服务名"; exit 1; }
BIN_PATH=$(readlink -f /proc/$PID/exe)
log "现有二进制: $BIN_PATH"

DATA_DIR=$(tr '\0' '\n' < /proc/$PID/environ | grep '^ZO_DATA_DIR=' | cut -d= -f2- || true)
log "数据目录 ZO_DATA_DIR=${DATA_DIR:-<默认，未显式设置>}  —— 本脚本【不会】触碰它"

mkdir -p "$BAK_DIR"
TS=$(date +%s)
cp -a "$BIN_PATH" "${BAK_DIR}/openobserve.bak.${TS}"
echo "$BIN_PATH" > "${BAK_DIR}/bin_path.txt"
log "旧二进制已备份到 ${BAK_DIR}/openobserve.bak.${TS}"

# ------------------------------------------------- 2. 写 systemd override
log "写入 SAML + cookie 环境变量 override: $OVERRIDE_FILE"
mkdir -p "$OVERRIDE_DIR"
cat > "$OVERRIDE_FILE" <<EOF
[Service]
Environment=ZO_SAML_ENABLED=true
Environment=ZO_SAML_IDP_SSO_URL=https://tai.it.tencent.com/api/saml2/sso
Environment=ZO_SAML_IDP_ENTITY_ID=https://tai.it.tencent.com/api/saml2/metadata
Environment=ZO_SAML_IDP_CERT=${IDP_CERT}
Environment=ZO_SAML_SP_ENTITY_ID=${SP_HOST}/config/saml/metadata
Environment=ZO_SAML_SP_ACS_URL=${SP_HOST}/config/saml/acs
Environment=ZO_SAML_DEFAULT_ORG=default
Environment=ZO_SAML_DEFAULT_ROLE=admin
Environment=ZO_COOKIE_SAME_SITE_LAX=false
Environment=ZO_COOKIE_SECURE_ONLY=true
EOF

# ------------------------------------------------- 3. 替换二进制 + 重启
log "停止服务并替换二进制"
systemctl stop "$SERVICE"
install -m 0755 "$NEW_BIN" "$BIN_PATH"
systemctl daemon-reload
systemctl start "$SERVICE"

# ------------------------------------------------- 4. 健康检查（失败回滚）
log "健康检查中..."
OK=0
for i in $(seq 1 20); do
  sleep 1
  if curl -s http://127.0.0.1:5080/config >/dev/null 2>&1; then OK=1; break; fi
done
if [[ "$OK" -ne 1 ]]; then
  err "服务启动后无响应，自动回滚！"
  systemctl stop "$SERVICE" || true
  install -m 0755 "${BAK_DIR}/openobserve.bak.${TS}" "$BIN_PATH"
  rm -f "$OVERRIDE_FILE"
  systemctl daemon-reload
  systemctl start "$SERVICE"
  err "已回滚到旧二进制。请把 journalctl -u ${SERVICE} 最近日志发回排查。"
  exit 1
fi

SSO=$(curl -s http://127.0.0.1:5080/config | grep -oE '"sso_type":"[^"]*"' || true)
log "服务已起，sso_type = ${SSO:-<未读到>}"
if echo "$SSO" | grep -q saml; then
  log "✅ SAML 已启用。接下来执行 nginx-125-setup.sh 装 443 反代，再去太湖注册 SP。"
else
  err "⚠️ sso_type 不是 saml，请检查 override 是否生效（systemctl show ${SERVICE} | grep ZO_SAML）"
fi
log "完成。回滚命令: sudo bash deploy-on-125.sh --rollback"
