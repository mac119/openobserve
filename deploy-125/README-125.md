# 在 43.128.101.125 部署「太湖 SAML 版」OpenObserve

> ⚠️ 生产机。本流程**只替换二进制 + 加环境变量 + 装 nginx**，脚本内置备份与自动回滚，
> **绝不触碰 ZO_DATA_DIR 数据目录和任何日志**。

## 制品清单（本目录 deploy-125/）

| 文件 | 作用 | 在哪执行 |
| --- | --- | --- |
| `deploy-on-125.sh` | 替换二进制 + 写 SAML/cookie systemd override + 重启 + 健康检查(失败自动回滚) | 43.128.101.125 |
| `nginx-125-setup.sh` | 装 Nginx 443 自签证书反代 → 5080 | 43.128.101.125 |
| `README-125.md` | 本说明 | — |

另外需要单独搬运的大文件（不在本目录）：
- 二进制 `openobserve`（361M）
  - 路径：编译机 `/home/ubuntu/leoy/monitor/openobserve/target/release/openobserve`
  - sha256：`aba143ae2dd75d0b6ccc8d9f67d28a865dd03783040e72e6bcfffbb4cff09a68`

## 步骤一：把文件搬到目标机

编译机(10.15.177.87)与目标机不互通，经你的通道（跳板/U盘/对象存储）把两样东西弄到 43.128.101.125：
1. 二进制 → 放到 `/tmp/openobserve-saml`
2. 本 `deploy-125/` 目录（两个 .sh）→ 放到目标机任意目录，例如 `~/deploy-125/`

搬运后在目标机校验二进制完整：
```bash
sha256sum /tmp/openobserve-saml
# 必须等于 aba143ae2dd75d0b6ccc8d9f67d28a865dd03783040e72e6bcfffbb4cff09a68
```

## 步骤二：部署二进制 + SAML 环境变量

```bash
cd ~/deploy-125
sudo bash deploy-on-125.sh
```
脚本会自动：校验 sha256/glibc/build_type → 备份旧二进制 → 停服务 → 换二进制 →
写 systemd override（SAML + cookie）→ 启动 → 健康检查。
**失败自动回滚**。成功会打印 `sso_type = "saml"`。

> 若服务名不是 `openobserve`，先改脚本顶部 `SERVICE=` 变量。
> 回滚：`sudo bash deploy-on-125.sh --rollback`

## 步骤三：装 Nginx 443 反代

```bash
sudo bash nginx-125-setup.sh
# 验证：
curl -sk https://127.0.0.1/config/saml/metadata | head -3   # 应返回 SP metadata XML
```
> 若该机 443 已被占用（如已有别的站点），`nginx -t` 会报错，先确认 443 空闲或告诉我现状。

## 步骤四：太湖后台再注册一个 SP（host 不同，必须新建）

在太湖用「方式二」填：
- ACS 地址：`https://43.128.101.125/config/saml/acs`
- SP Entity ID：`https://43.128.101.125/config/saml/metadata`
- SubjectNameID：固定值 / **EmailAddress** / 邮箱-eduPersonPrincipalName

## 步骤五：腾讯云安全组放通 443 入站

## 步骤六：验证登录

浏览器开 `https://43.128.101.125/web/login`（自签证书点继续）→ "Login with SSO"
→ 太湖登录 → 进入 Dashboard。

## 出问题时抓日志（发回排查）

```bash
# 服务日志
journalctl -u openobserve --no-pager | tail -50
# SAML/验签相关（systemd 日志）
journalctl -u openobserve --no-pager | grep -aiE "saml|assertion|signature|digest|provision" | tail -30
# 确认 SAML 变量已注入
systemctl show openobserve | grep ZO_SAML
```

## 关键差异（相对 43.163.110.244 那台）

- SP Entity ID / ACS URL 的 host 改为 `43.128.101.125`
- 太湖需**另注册一个 SP**（SAML 按 ACS/entityID 区分，不能共用）
- root 账号、数据目录等**全部沿用目标机原有 systemd 配置**，脚本不改
