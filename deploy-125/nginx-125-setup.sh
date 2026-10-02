#!/usr/bin/env bash
#
# 在 43.128.101.125 上部署 Nginx 443 TLS 反代 -> 127.0.0.1:5080
# 太湖 SAML 回调要求 https，本脚本用自签证书终止 TLS。
#
# 用法：sudo bash nginx-125-setup.sh
set -euo pipefail

HOST_IP="43.128.101.125"
SSL_DIR="/etc/nginx/ssl"
CONF="/etc/nginx/sites-available/openobserve-ssl.conf"

log(){ echo -e "\033[1;32m[nginx]\033[0m $*"; }
err(){ echo -e "\033[1;31m[error]\033[0m $*" >&2; }
[[ $EUID -ne 0 ]] && { err "请用 sudo 运行"; exit 1; }

# 1. 安装 nginx（若未装）
if ! command -v nginx >/dev/null 2>&1; then
  log "安装 nginx"
  (apt-get update && apt-get install -y nginx) || { err "apt 安装失败，请手动装 nginx 后重跑"; exit 1; }
fi

# 2. 自签证书（SAN = 本机外网 IP，10 年）
mkdir -p "$SSL_DIR"
if [[ ! -f "$SSL_DIR/o2-selfsigned.crt" ]]; then
  log "生成自签证书 (CN=$HOST_IP)"
  openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$SSL_DIR/o2-selfsigned.key" \
    -out "$SSL_DIR/o2-selfsigned.crt" \
    -days 3650 -subj "/CN=$HOST_IP" \
    -addext "subjectAltName=IP:$HOST_IP"
else
  log "证书已存在，复用"
fi

# 3. 反代配置
log "写入反代配置 $CONF"
cat > "$CONF" <<'EOF'
server {
    listen 443 ssl;
    server_name 43.128.101.125;

    ssl_certificate     /etc/nginx/ssl/o2-selfsigned.crt;
    ssl_certificate_key /etc/nginx/ssl/o2-selfsigned.key;
    ssl_protocols TLSv1.2 TLSv1.3;

    client_max_body_size 0;

    location / {
        proxy_pass http://127.0.0.1:5080;
        proxy_http_version 1.1;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-Host  $host;
        proxy_set_header Upgrade           $http_upgrade;
        proxy_set_header Connection        "upgrade";
        proxy_read_timeout 600s;
        proxy_send_timeout 600s;
        proxy_buffering off;
    }
}
EOF

# 4. 启用（注意：若该机 443 已被占用，脚本会在 nginx -t 报错，请先确认）
ln -sf "$CONF" /etc/nginx/sites-enabled/openobserve-ssl.conf

# 若默认站点也监听 443 default_server 会冲突，这里不动默认站点，仅本 server 不加 default_server
log "测试 nginx 配置"
nginx -t
systemctl reload nginx || systemctl restart nginx
log "443 监听情况:"
ss -ltnp | grep ':443 ' || err "443 未监听，请检查"
log "完成。验证: curl -sk https://127.0.0.1/config/saml/metadata | head -3"
