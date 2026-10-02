# OpenObserve 太湖 (Taihu) SAML 2.0 SSO 集成

本文档描述在 **开源版 OpenObserve 源码** 中新增的 SAML 2.0 Service Provider (SP)
登录能力，用于对接腾讯内部太湖 (Taihu) IdP。实现为 Rust 原生、单体集成，不依赖
闭源企业版 crate，仅在 **非 enterprise** 构建中启用。

## 1. 背景

- 太湖是 **SAML 2.0 IdP**（不是 OIDC）。IdP 元数据要点：
  - SSO 端点：`https://tai.it.tencent.com/api/saml2/sso`（HTTP-Redirect / HTTP-POST）
  - NameID 格式：`transient`（名义不稳定，**不**作为持久用户标识）
  - 对 assertion 签名（SP 侧 `WantAssertionsSigned=true`）
- 用户标识优先取 attributes 中的 **eduPersonPrincipalName（邮箱）**，
  OID `urn:oid:1.3.6.1.4.1.5923.1.1.1.6`；回退 uid / NameID。

## 2. 登录流程

```
浏览器 → GET  /config/saml/login     (SP 构造 AuthnRequest, 302 跳转太湖)
       → 太湖登录 (IOA 准入)
       → POST /config/saml/acs       (太湖回调, 携带 SAMLResponse)
            SP: 验签 assertion → 提取邮箱 → 自动建用户 → 签发 session cookie
       → 302 /web/                    (登录完成)
```

两条登录路径（密码 + SAML）最终都收敛到 OpenObserve 既有的 session 机制：
ACS 成功后生成随机 `secret` 写入用户 `password_ext`，并以 `session <id>` cookie
的形式交给浏览器（与开源 OIDC 实现同一套 session/建用户逻辑）。

## 3. 代码改动

| 文件 | 改动 |
| --- | --- |
| `src/config/src/config.rs` | `Auth` 新增 `ZO_SAML_*` 配置项 |
| `src/service/saml.rs` | **新增**：SP metadata、AuthnRequest(Redirect binding)、SAMLResponse 解析、**assertion 签名验证（exclusive-C14N + RSA PKCS#1 v1.5）**、属性提取 |
| `src/handler/http/request/saml_login.rs` | **新增**：`/config/saml/metadata`、`/config/saml/login`、`/config/saml/acs` 三个路由；ACS 复用 `ensure_user` + `set_session` |
| `src/handler/http/router/mod.rs` | 开源 `config_routes` 注册上述三个路由 |
| `src/handler/http/request/status/mod.rs` | `sso_enabled` 纳入 `saml_enabled`；新增 `sso_type`("saml"/"oidc"/"") |
| `web/src/components/login/Login.vue` | SSO 按钮按 `sso_type` 分流：`saml` 直接跳 `/config/saml/login` |
| `Cargo.toml` | 复用依赖树已有：`roxmltree`、`quick-xml`、`rsa`、`sha1`/`sha2`(oid feature) |

## 4. 配置项（环境变量）

| 变量 | 说明 | 示例 |
| --- | --- | --- |
| `ZO_SAML_ENABLED` | 开关 | `true` |
| `ZO_SAML_IDP_SSO_URL` | IdP SSO 端点 | `https://tai.it.tencent.com/api/saml2/sso` |
| `ZO_SAML_IDP_ENTITY_ID` | IdP entityID | `https://tai.it.tencent.com/api/saml2/metadata` |
| `ZO_SAML_IDP_CERT` | IdP 签名证书（base64 DER，即 IdP metadata 里 `<X509Certificate>` 内容） | `MIIDGTCC...` |
| `ZO_SAML_SP_ENTITY_ID` | 本 SP entityID | `http://43.163.110.244:5080/config/saml/metadata` |
| `ZO_SAML_SP_ACS_URL` | 本 SP ACS 回调 | `http://43.163.110.244:5080/config/saml/acs` |
| `ZO_SAML_ATTR_EMAIL` | 邮箱属性名 | 默认 `urn:oid:1.3.6.1.4.1.5923.1.1.1.6` |
| `ZO_SAML_ATTR_NAME` | 显示名属性名 | 默认 `urn:oid:2.5.4.3` |
| `ZO_SAML_DEFAULT_ORG` | 首登建用户所属 org | `default` |
| `ZO_SAML_DEFAULT_ROLE` | 首登建用户角色 | `admin` |

> IdP 证书填写：把太湖 IdP metadata 里 `<X509Certificate>...</X509Certificate>`
> 标签内的 base64 整段（去掉换行）填入 `ZO_SAML_IDP_CERT` 即可，带不带 PEM 头均可。

## 5. 在太湖注册 SP

1. 启动 OpenObserve 后访问 `GET /config/saml/metadata` 获取本 SP 的 metadata XML。
2. 在太湖管理后台注册该 SP：
   - entityID = `ZO_SAML_SP_ENTITY_ID`
   - ACS (HTTP-POST) = `ZO_SAML_SP_ACS_URL`
   - NameIDFormat = transient
   - 要求 IdP 对 assertion 签名（WantAssertionsSigned=true）
3. 把太湖返回的 IdP 签名证书配到 `ZO_SAML_IDP_CERT`。

## 6. 安全设计

- **验签是强制的**：ACS 收到 SAMLResponse 后，必须先通过 `verify_and_extract`
  校验 assertion 的 XML-DSig 签名，才提取身份、建用户、发 session。验证两步：
  1. Reference digest：对 Assertion 做 **enveloped-signature transform（移除签名）
     + exclusive-C14N**，计算摘要并与 `DigestValue` 比对；
  2. Signature：对 `SignedInfo` 做 exclusive-C14N，用 IdP 证书公钥做 RSA PKCS#1 v1.5
     验签（SHA-1 / SHA-256 由 `SignatureMethod` 决定）。
- `AuthnRequestsSigned=false`（SP 不签名请求）、不声明加密 KeyDescriptor。
- Status 必须为 `Success`；Reference URI 必须指向被验证的 Assertion ID。

## 7. 已知事项 / 待实测校准

- **XML 规范化 (exclusive-C14N)** 自实现，覆盖标准 SAML assertion 结构。按 taihu.txt
  的经验（"实测，不要假设"），**务必用一次真实太湖 SAMLResponse 做验签联调**：
  若 digest/signature 校验失败，通常是 C14N 的 namespace 渲染或属性排序与 IdP 侧
  细节不一致，需用真实样本比对 `SignedInfo`/`Assertion` 的 c14n 输出来定位。
- 未实现 SLO（单点登出）、assertion 加密（太湖侧未启用）、SAMLResponse 外层签名
  （仅验证 assertion 签名；如太湖对 Response 整体签名而非 assertion，需扩展定位逻辑）。
- `InclusiveNamespaces PrefixList` 未特殊处理（标准 exc-c14n 无 PrefixList 的场景）。

## 8. 验证步骤

```bash
# 1. 配置环境变量（见 §4），启动 openobserve（开源构建）
ZO_SAML_ENABLED=true ZO_SAML_IDP_SSO_URL=... ZO_SAML_IDP_CERT=... \
ZO_SAML_SP_ENTITY_ID=... ZO_SAML_SP_ACS_URL=... ./openobserve

# 2. 取 SP metadata 注册到太湖
curl http://<host>:5080/config/saml/metadata

# 3. 浏览器打开登录页，点 "Login with SSO" → 跳转太湖 → 登录 → 回跳 /web/
```
