# OpenObserve 定制版 Handover 文档

> 面向：接手这套 OpenObserve 定制代码、在与 `43.128.101.125` 同 VPC 的新机器上继续开发/部署的同事。
> 日期：2026-10-02
> 代码基线：git `6f1f36121`（"Initial commit"，分支 `feat/3398-colorize-log-rows`）

---

## 0. TL;DR

这是一套**基于 OpenObserve 开源版（opensource build）二次开发**的定制版本，主要增加了：

1. **太湖(Taihu) SAML 2.0 SSO 登录**（本次核心，Rust 原生实现，无需企业版 crate）——**代码改动尚未 git commit，在工作区**。
2. 之前已完成并已在 Initial commit 内的改动：解除 50GB/天摄入限制、前端定制（隐藏企业按钮/显示 O2 Assistant/移除 Slack）、标准 OIDC SSO。

当前两套线上环境：
- `43.163.110.244`（内网 `10.15.177.87` 编译机本机）：**太湖 SAML 已部署成功、可登录**。
- `43.128.101.125`：**待部署**（与 244 网络不通，故需迁移代码到同 VPC 机器后再部署）。

---

## 1. 环境清单

| 环境 | 说明 | 状态 |
| --- | --- | --- |
| `43.163.110.244`（= 内网 `10.15.177.87`） | 本编译机，opensource，手动 `./target/release/openobserve` 启动 | 太湖 SAML 已跑通 ✅ |
| `43.128.101.125` | 生产机，opensource，systemd 启动，已有 nginx(8088 统一入口) | 待部署太湖 SAML |
| `172.22.16.10` | 另一套**企业版** v0.91.1（独立实例，与上面无关） | 仅摄入用，不在本次范围 |

> 太湖 IdP：`https://tai.it.tencent.com/api/saml2/sso`（SAML 2.0），两套 SP 共用同一 IdP 证书。

---

## 2. 本次核心：太湖 SAML 2.0 SSO 集成

### 2.1 为什么是 SAML（不是 OIDC）
太湖实际是 **SAML 2.0 IdP**（见 `taihu.txt` 与 `metadata.xml`）。之前做过的标准 OIDC 实现对太湖**用不上**，因此新写了一套 SAML 2.0 SP。

### 2.2 登录流程
```
浏览器 → GET  /config/saml/login     (SP 构造 AuthnRequest, 302 跳太湖)
       → 太湖登录(IOA 准入)
       → POST /config/saml/acs       (太湖回调, 带 SAMLResponse)
            SP: 验签 assertion → 取邮箱 → 自动建用户 → 签发 session cookie
                → 302 到 /web/login#id_token=<前端JWT>
       → 前端据 #id_token 建立登录态 → 进入 Dashboard
```

### 2.3 用户标识
优先取 assertion 属性 `eduPersonPrincipalName`（邮箱，OID `urn:oid:1.3.6.1.4.1.5923.1.1.1.6`），兜底 NameID。

### 2.4 代码改动清单（本次 SAML，**工作区未提交**）

| 文件 | 改动 |
| --- | --- |
| `src/service/saml.rs` | **新增（核心）**：SP metadata、AuthnRequest(HTTP-Redirect: DEFLATE+base64+urlencode)、SAMLResponse 解析、**assertion 签名验证（exclusive-C14N + RSA PKCS#1 v1.5, SHA1/256）**、属性提取。C14N 为自实现。 |
| `src/handler/http/request/saml_login.rs` | **新增**：`/config/saml/metadata`、`/config/saml/login`、`/config/saml/acs` 三路由；ACS 验签→`ensure_user`→`set_session`→跳 `#id_token` |
| `src/config/src/config.rs` | `Auth` 新增 10 个 `ZO_SAML_*` 配置项 |
| `src/service/mod.rs` | 注册 `pub mod saml;`（`#[cfg(not(enterprise))]`） |
| `src/handler/http/request/mod.rs` | 注册 `pub mod saml_login;`（`#[cfg(not(enterprise))]`） |
| `src/handler/http/router/mod.rs` | 开源 `config_routes()` 注册 3 个 SAML 路由（**必须放在 `route_layer(auth_middleware)` 之后**，否则被鉴权拦截→401） |
| `src/handler/http/request/status/mod.rs` | `sso_enabled` 纳入 `saml_enabled`；新增 `sso_type`("saml"/"oidc"/"") 供前端分流 |
| `web/src/components/login/Login.vue` | SSO 按钮按 `sso_type` 分流：`saml` 直接 `window.location` 跳 `/config/saml/login` |
| `Cargo.toml` / `Cargo.lock` | 新增依赖（均复用依赖树已有，无新来源）：`roxmltree`、`quick-xml`、`rsa`、`sha1`(oid)、`sha2`(oid) |

### 2.5 配置项（环境变量）

| 变量 | 说明 | 示例/默认 |
| --- | --- | --- |
| `ZO_SAML_ENABLED` | 总开关 | `true` |
| `ZO_SAML_IDP_SSO_URL` | 太湖 SSO 端点 | `https://tai.it.tencent.com/api/saml2/sso` |
| `ZO_SAML_IDP_ENTITY_ID` | 太湖 entityID | `https://tai.it.tencent.com/api/saml2/metadata` |
| `ZO_SAML_IDP_CERT` | 太湖签名证书(base64 DER，见 metadata.xml 的 `<X509Certificate use="signing">`) | `MIIDGTCC...` |
| `ZO_SAML_SP_ENTITY_ID` | 本 SP entityID（**每台不同**） | `https://<host>/config/saml/metadata` |
| `ZO_SAML_SP_ACS_URL` | 本 SP ACS（**每台不同**） | `https://<host>/config/saml/acs` |
| `ZO_SAML_ATTR_EMAIL` | 邮箱属性名 | 默认 `urn:oid:1.3.6.1.4.1.5923.1.1.1.6` |
| `ZO_SAML_ATTR_NAME` | 显示名属性名 | 默认 `urn:oid:2.5.4.3` |
| `ZO_SAML_DEFAULT_ORG` | 首登建用户 org | `default` |
| `ZO_SAML_DEFAULT_ROLE` | 首登建用户角色 | `admin` |
| `ZO_COOKIE_SAME_SITE_LAX` | **必须 `false`**（跨站 POST 回调需 SameSite=None） | `false` |
| `ZO_COOKIE_SECURE_ONLY` | **必须 `true`**（SameSite=None 要求 Secure，配合 https） | `true` |

> 完整示例见根目录 `.o2_saml.env`（⚠️ 含 root 密码与证书，属敏感文件）。

---

## 3. 之前已完成（已在 Initial commit 内）

| 功能 | 关键文件 |
| --- | --- |
| 解除 50GB/天摄入限制 + 去 UI 提示 | `src/service/search/mod.rs`（`check_search_allowed` 直接 Ok）、`src/handler/http/request/license/mod.rs` |
| 标准 OIDC SSO（PKCE） | `src/service/oidc.rs`、`src/handler/http/request/oidc_login.rs` |
| 前端定制（隐藏企业按钮/O2 Assistant tab/移除 Slack） | `web/src/components/Header.vue`、`web/src/views/HomeView.vue` |
| 开源版 session/cookie 认证打通 | `src/common/utils/auth.rs`（`from_request_parts` 支持 `session <id>` 还原）、`src/service/session.rs` |

---

## 4. 编译步骤

前端是 `rust_embed_for_web` **编译期**嵌入（`src/handler/http/router/ui.rs` 的 `#[folder="web/dist/"]`），**改前端必须先构建前端再重编后端**。

```bash
# 1) 前端（改了 web/ 才需要）
cd web && NODE_OPTIONS="--max_old_space_size=8192" npm run build && cd ..
# 2) 后端（touch build.rs 触发重新嵌入 dist）
touch build.rs
cargo build --release --bin openobserve
# 产物：target/release/openobserve （单文件，已含前端）
```

- 工具链：Rust（见 `rust-toolchain.toml`）、Node 20 / npm 10（前端）。
- glibc：本编译机 2.35（Ubuntu 22.04）。**部署目标机 glibc 必须 ≥ 2.35**，否则动态链接二进制跑不起来 → 需在目标机或同 glibc 环境重编。
- 全量 release 编译约 **13–17 分钟**。

---

## 5. 部署步骤（以 125 为例）

### 5.1 二进制 + 环境变量（systemd）
见 `deploy-125/`：
- `deploy-on-125.sh`：校验 sha256/glibc/build_type → 备份旧二进制 → 停服务 → 换二进制 → 写 systemd override(SAML+cookie) → 启动 → 健康检查(失败自动回滚)。**不碰数据/日志**。
- 回滚：`sudo bash deploy-on-125.sh --rollback`

### 5.2 Nginx 443（SAML 必须 https）
125 已有 nginx（统一入口在 **8088**：AI chat→`8799`，其余→`5080`）。443 空闲。
**新增独立 443 server 块**（不要用 `deploy-125/nginx-125-setup.sh`，它没有 AI chat 路由），复刻 8088 路由 + TLS，写到 `/etc/nginx/conf.d/openobserve-ssl.conf`：
- `location ~ ^/api/[^/]+/ai/chat_stream$` → `127.0.0.1:8799`
- `location /` → `127.0.0.1:5080`
- 两者都加 `proxy_set_header X-Forwarded-Proto $scheme;`
- 自签证书 SAN=对外 IP/域名。

### 5.3 太湖后台注册 SP（每台一个，按 ACS 区分）
- ACS：`https://<host>/config/saml/acs`
- SP Entity ID：`https://<host>/config/saml/metadata`
- SubjectNameID：固定值 / **EmailAddress** / 邮箱-eduPersonPrincipalName
- 提交后可下载太湖 IdP metadata 核对签名证书（两套 SP 共用同一 IdP，证书相同）。

### 5.4 安全组放通 **443** 入站。

### 5.5 验证
`https://<host>/web/login` → "Login with SSO" → 太湖登录 → 进 Dashboard。

---

## 6. 关键踩坑记录（务必读）

1. **太湖是 SAML 不是 OIDC** —— 不要再往 OIDC 方向折腾。
2. **axum `route_layer` 语义**：`route_layer(auth_middleware)` 只作用于**之前**注册的路由。SAML 入口路由必须放在它**之后**，否则未认证访问被拦 401。（已修正）
3. **前端登录态靠 `#id_token` fragment**：OpenObserve 前端 `views/Login.vue` 的 `created()` 读 `location.hash` 的 `id_token` 建立登录态。ACS 不能裸跳 `/web/`，必须跳 `/web/login#id_token=<JWT>`（JWT 仅供前端，payload 含 email/name + `pgdata` 标记以跳过开源版 `VerifyAndCreateUser`）。API 鉴权另走 cookie。（已修正）
4. **Cookie SameSite**：太湖是跨站 POST 回调 ACS，`SameSite=Lax`(默认) 会导致 cookie 不被保存 → 打回登录页。必须 `ZO_COOKIE_SAME_SITE_LAX=false` + `ZO_COOKIE_SECURE_ONLY=true`（需 https）。（已修正）
5. **前端 API_ENDPOINT**：`Login.vue` 里不能用 `config.API_ENDPOINT`（可能 undefined，拼出 `/web/undefined/...`），要用 `store.state.API_ENDPOINT || window.location.origin`。（已修正）
6. **XML 规范化(C14N)是自实现**：`src/service/saml.rs` 的 `mod c14n` 实现 exclusive-C14N。**若真实太湖响应验签失败**（`assertion digest mismatch` / `signature verification failed`），多半是 C14N 的 namespace 渲染/属性排序与 IdP 细节不一致——需用**真实 SAMLResponse 样本**比对 `SignedInfo`/`Assertion` 的 c14n 输出来定位。目前 244 实测可通过。
7. **base64 命名遮蔽**：`saml_login.rs` 顶部 `use config::utils::base64` 遮蔽了外部 crate，编码字节要用 `::base64::`。
8. **rust-embed 编译期嵌入**：改前端必须重构建前端 + `touch build.rs` 重编后端，无运行时读盘模式。

---

## 7. 文件/制品清单

| 路径 | 说明 | 是否在源码包 |
| --- | --- | --- |
| `src/service/saml.rs` | SAML 核心实现 | ✅ |
| `src/handler/http/request/saml_login.rs` | SAML 路由 handler | ✅ |
| `SAML_TAIHU_INTEGRATION.md` | SAML 设计/配置文档 | ✅ |
| `HANDOVER.md` | 本文档 | ✅ |
| `taihu.txt` | 太湖接入经验资料 | ✅ |
| `metadata.xml` | 太湖 IdP metadata（含签名证书） | ✅ |
| `.o2_saml.env` | 启动环境变量模板（⚠️ 含密码/证书） | ✅（敏感） |
| `deploy-125/` | 125 部署包（脚本）。**二进制 `openobserve-saml` 361M 不进源码包** | 仅脚本 |
| `openobserve-taihu-125.tar.gz` | 125 一键部署包(139M, 含二进制) | ❌ 另传 |
| `disable_802/`, `disable_802.zip` | SAML 参考实现(Python)，256M | ❌ 另传 |
| `target/`, `web/node_modules/` | 构建产物/依赖 | ❌ 目标机重建 |

---

## 8. 迁移到新机后的 checklist

1. 解压源码包，`git status` 确认 SAML 改动都在（`src/service/saml.rs` 等）。
2. 装工具链：Rust(见 rust-toolchain.toml) + Node20/npm10。确认 **glibc ≥ 2.35**。
3. 编译：`cd web && npm ci && npm run build && cd .. && cargo build --release`（首次编译很久）。
4. 本地自测：设 `ZO_SAML_*` + root 账号起服务，`curl /config | grep sso_type` 应为 `saml`。
5. 按 §5 部署到 125（新机与 125 同 VPC，scp 应可直达）。
6. 太湖注册 SP + 安全组 443 + 浏览器实测登录。
7. 若验签失败 → 抓真实 SAMLResponse 按 §6.6 校准 C14N。

---

## 9. 敏感信息提醒

- `.o2_saml.env` 含 `ZO_ROOT_USER_PASSWORD`、太湖证书等，迁移后请妥善保管、勿入公开仓库。
- 自签证书仅用于 TLS 终止，浏览器会提示风险；生产建议换 CA 证书或用域名。
- 太湖后台给的 app token（应用密钥）**SAML 流程不使用**，如无其他用途可忽略；相关密钥请存于内部密钥管理，勿写入仓库。
