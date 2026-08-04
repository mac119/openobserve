# OpenObserve 后端 API 文档

> 基于 `src/handler/http/router/mod.rs` 路由定义自动整理  
> 生成时间: 2026-05-07

---

## 目录

- [1. 基础路由（无需认证）](#1-基础路由无需认证)
- [2. 认证路由](#2-认证路由)
- [3. 节点管理](#3-节点管理)
- [4. 配置路由](#4-配置路由)
- [5. 用户管理](#5-用户管理)
- [6. 组织管理](#6-组织管理)
- [7. 系统设置 v2](#7-系统设置-v2)
- [8. ES 兼容接口](#8-es-兼容接口)
- [9. 数据流（Streams）](#9-数据流streams)
- [10. 日志摄入（Logs Ingestion）](#10-日志摄入logs-ingestion)
- [11. 链路追踪（Traces）](#11-链路追踪traces)
- [12. 指标摄入（Metrics）](#12-指标摄入metrics)
- [13. PromQL 查询](#13-promql-查询)
- [14. 搜索（Search）](#14-搜索search)
- [15. 多流搜索](#15-多流搜索)
- [16. HTTP/2 流式搜索](#16-http2-流式搜索)
- [17. 保存视图（Saved Views）](#17-保存视图saved-views)
- [18. 函数（Functions）](#18-函数functions)
- [19. 仪表盘（Dashboards）](#19-仪表盘dashboards)
- [20. 报告（Reports）](#20-报告reports)
- [21. 定时注解（Timed Annotations）](#21-定时注解timed-annotations)
- [22. 报告 v2](#22-报告-v2)
- [23. 文件夹 v2（Folders）](#23-文件夹-v2folders)
- [24. 告警 v2（Alerts）](#24-告警-v2alerts)
- [25. 告警事件（Incidents）](#25-告警事件incidents)
- [26. 告警模板（Alert Templates）](#26-告警模板alert-templates)
- [27. 告警目标（Alert Destinations）](#27-告警目标alert-destinations)
- [28. 去重（Deduplication）](#28-去重deduplication)
- [29. KV 存储](#29-kv-存储)
- [30. 富化表（Enrichment Tables）](#30-富化表enrichment-tables)
- [31. 权限管理（Authz/FGA）](#31-权限管理authzfga)
- [32. 集群管理](#32-集群管理)
- [33. 流水线（Pipelines）](#33-流水线pipelines)
- [34. 流水线回填（Pipeline Backfills）](#34-流水线回填pipeline-backfills)
- [35. 短链接](#35-短链接)
- [36. 服务账号](#36-服务账号)
- [37. MCP](#37-mcp)
- [38. Sourcemaps](#38-sourcemaps)
- [39. AWS 集成](#39-aws-集成)
- [40. GCP 集成](#40-gcp-集成)
- [41. RUM 集成](#41-rum-集成)
- [42. 代理路由（Proxy）](#42-代理路由proxy)
- [Enterprise 功能](#enterprise-功能)

---

## 1. 基础路由（无需认证）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/healthz` | 健康检查 |
| `HEAD` | `/healthz` | 健康检查（HEAD） |
| `GET` | `/schedulez` | 调度状态 |
| `GET` | `/metrics` | Prometheus 指标导出 |
| `GET` | `/.well-known/oauth-authorization-server` | OAuth 2.0 授权服务器元数据 |

---

## 2. 认证路由

前缀: `/auth`

| 方法 | 路径 | 说明 | 参数 |
|------|------|------|------|
| `POST` | `/auth/login` | 用户登录认证 | Body: `{ email, password }` |
| `GET` | `/auth/login` | 获取认证信息 | - |
| `GET` | `/auth/presigned-url` | 获取预签名 URL | - |
| `GET` | `/auth/invites` | 列出邀请列表 | - |
| `DELETE` | `/auth/invites/{token}` | 拒绝邀请 | Path: `token` |

---

## 3. 节点管理

前缀: `/node` （需认证）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/node/status` | 缓存状态 |
| `PUT` | `/node/enable` | 启用节点 |
| `PUT` | `/node/flush` | 刷新节点 |
| `GET` | `/node/reload` | 重载缓存 |
| `GET` | `/node/list` | 节点列表 |
| `GET` | `/node/metrics` | 节点指标 |
| `POST` | `/node/consistent_hash` | 一致性哈希计算 |
| `GET` | `/node/refresh_nodes_list` | 刷新节点列表 |
| `GET` | `/node/refresh_user_sessions` | 刷新用户会话 |
| `GET` | `/node/drain_status` | 排水状态 *(enterprise)* |

---

## 4. 配置路由

前缀: `/config`

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/config/` | 获取系统配置 |
| `GET` | `/config/logout` | 登出 |
| `GET` | `/config/runtime` | 运行时配置 |
| `GET` | `/config/reload` | 重载配置 |
| `GET` | `/config/redirect` | 重定向 *(enterprise)* |
| `GET` | `/config/dex_login` | Dex 登录 *(enterprise)* |
| `GET` | `/config/dex_refresh` | Dex Token 刷新 *(enterprise)* |
| `POST` | `/config/token` | Token 交换 *(enterprise)* |

---

## 5. 用户管理

前缀: `/api/{org_id}`

| 方法 | 路径 | 说明 | 路径参数 |
|------|------|------|----------|
| `GET` | `/api/{org_id}/users` | 列出用户 | `org_id` |
| `POST` | `/api/{org_id}/users` | 创建用户 | `org_id` |
| `POST` | `/api/{org_id}/users/{email_id}` | 添加用户到组织 | `org_id`, `email_id` |
| `PUT` | `/api/{org_id}/users/{email_id}` | 更新用户 | `org_id`, `email_id` |
| `DELETE` | `/api/{org_id}/users/{email_id}` | 删除用户 | `org_id`, `email_id` |
| `DELETE` | `/api/{org_id}/users/bulk` | 批量删除用户 | `org_id` |
| `GET` | `/api/{org_id}/users/roles` | 列出角色 | `org_id` |

---

## 6. 组织管理

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/organizations` | 列出组织 |
| `POST` | `/api/organizations` | 创建组织 |
| `POST` | `/api/{org_id}/organizations/assume_service_account` | 代理服务账户 |
| `GET` | `/api/{org_id}/settings` | 获取组织设置 |
| `POST` | `/api/{org_id}/settings` | 创建组织设置 |
| `POST` | `/api/{org_id}/settings/logo` | 上传 Logo |
| `DELETE` | `/api/{org_id}/settings/logo` | 删除 Logo |
| `POST` | `/api/{org_id}/settings/logo/text` | 设置 Logo 文字 |
| `DELETE` | `/api/{org_id}/settings/logo/text` | 删除 Logo 文字 |
| `GET` | `/api/{org_id}/summary` | 组织摘要 |
| `GET` | `/api/{org_id}/passcode` | 获取用户密码 |
| `PUT` | `/api/{org_id}/passcode` | 更新用户密码 |
| `GET` | `/api/{org_id}/rumtoken` | 获取 RUM Token |
| `POST` | `/api/{org_id}/rumtoken` | 创建 RUM Token |
| `PUT` | `/api/{org_id}/rumtoken` | 更新 RUM Token |
| `GET` | `/api/{org_id}/node/list` | 组织节点列表 |
| `GET` | `/api/{org_id}/cluster/info` | 集群信息 |
| `PUT` | `/api/{org_id}/rename` | 重命名组织 |

---

## 7. 系统设置 v2

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/settings/v2` | 列出所有设置 |
| `POST` | `/api/{org_id}/settings/v2` | 设置组织级别配置 |
| `GET` | `/api/{org_id}/settings/v2/{key}` | 获取单个设置 |
| `DELETE` | `/api/{org_id}/settings/v2/{key}` | 删除组织级别设置 |
| `POST` | `/api/{org_id}/settings/v2/user/{user_id}` | 设置用户级别配置 |
| `DELETE` | `/api/{org_id}/settings/v2/user/{user_id}/{key}` | 删除用户级别设置 |

---

## 8. ES 兼容接口

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET`/`HEAD` | `/api/{org_id}/` | 组织索引 |
| `GET` | `/api/{org_id}/_license` | License 信息 |
| `GET` | `/api/{org_id}/_xpack` | XPack 信息 |
| `GET`/`HEAD` | `/api/{org_id}/_ilm/policy/{name}` | ILM 策略 |
| `GET`/`HEAD`/`POST` | `/api/{org_id}/_index_template/{name}` | 索引模板 |
| `GET`/`HEAD`/`POST` | `/api/{org_id}/_data_stream/{name}` | 数据流 |
| `GET`/`HEAD`/`POST` | `/api/{org_id}/_ingest/pipeline/{name}` | 摄入管道 |

---

## 9. 数据流（Streams）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/streams` | 列出流 |
| `POST` | `/api/{org_id}/streams/{stream_name}` | 创建流 |
| `DELETE` | `/api/{org_id}/streams/{stream_name}` | 删除流 |
| `GET` | `/api/{org_id}/streams/{stream_name}/schema` | 获取流 Schema |
| `PUT` | `/api/{org_id}/streams/{stream_name}/settings` | 更新流设置 |
| `PUT` | `/api/{org_id}/streams/{stream_name}/update_fields` | 更新字段 |
| `PUT` | `/api/{org_id}/streams/{stream_name}/delete_fields` | 删除字段 |
| `DELETE` | `/api/{org_id}/streams/{stream_name}/cache/results` | 删除流缓存 |
| `DELETE` | `/api/{org_id}/streams/{stream_name}/data_by_time_range` | 按时间范围删除数据 |
| `GET` | `/api/{org_id}/streams/{stream_name}/data_by_time_range/status/{id}` | 获取删除状态 |

---

## 10. 日志摄入（Logs Ingestion）

| 方法 | 路径 | 说明 | Content-Type |
|------|------|------|-------------|
| `POST` | `/api/{org_id}/_bulk` | ES Bulk 写入 | `application/x-ndjson` |
| `POST` | `/api/{org_id}/{stream_name}/_multi` | 多行写入 | `application/json` |
| `POST` | `/api/{org_id}/{stream_name}/_json` | JSON 写入 | `application/json` |
| `POST` | `/api/{org_id}/_hec` | Splunk HEC 兼容写入 | `application/json` |
| `POST` | `/api/{org_id}/loki/api/v1/push` | Loki 兼容推送 | `application/json` |
| `POST` | `/api/{org_id}/v1/logs` | OTLP Logs 写入 | `application/x-protobuf` |

---

## 11. 链路追踪（Traces）

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/v1/traces` | OTLP Traces 写入 |
| `POST` | `/api/{org_id}/traces` | Traces 写入 |
| `POST` | `/api/{org_id}/otel/v1/traces` | OTel Traces 写入 |
| `GET` | `/api/{org_id}/{stream_name}/traces/latest` | 获取最新 Traces |
| `GET` | `/api/{org_id}/{stream_name}/traces/latest_stream` | 获取最新 Traces（流式） |
| `GET` | `/api/{org_id}/{stream_name}/traces/session` | 获取最新会话 |
| `GET` | `/api/{org_id}/{stream_name}/traces/user` | 获取最新用户 |
| `GET` | `/api/{org_id}/{stream_name}/traces/{trace_id}/dag` | 获取 Trace DAG |

---

## 12. 指标摄入（Metrics）

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/v1/metrics` | OTLP Metrics 写入 |
| `POST` | `/api/{org_id}/ingest/metrics/_json` | JSON Metrics 写入 |

---

## 13. PromQL 查询

前缀: `/api/{org_id}/prometheus/api/v1`

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `.../write` | Remote Write |
| `GET`/`POST` | `.../query` | 即时查询 |
| `GET`/`POST` | `.../query_range` | 范围查询 |
| `GET`/`POST` | `.../query_exemplars` | Exemplars 查询 |
| `GET` | `.../metadata` | 指标元数据 |
| `GET`/`POST` | `.../series` | Series 查询 |
| `GET`/`POST` | `.../labels` | Labels 列表 |
| `GET` | `.../label/{label_name}/values` | Label Values |
| `GET`/`POST` | `.../format_query` | 格式化查询 |

---

## 14. 搜索（Search）

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/_search` | 搜索 |
| `POST` | `/api/{org_id}/_search_partition` | 搜索分区 |
| `GET` | `/api/{org_id}/{stream_name}/_around` | Around 查询 v1 |
| `POST` | `/api/{org_id}/{stream_name}/_around` | Around 查询 v2 |
| `GET` | `/api/{org_id}/{stream_name}/_values` | Values 查询 |
| `POST` | `/api/{org_id}/_search_history` | 搜索历史 |
| `POST` | `/api/{org_id}/result_schema` | 结果 Schema |

---

## 15. 多流搜索

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/_search_multi` | 多流搜索 |
| `POST` | `/api/{org_id}/_search_multi_stream` | 多流搜索（流式） |
| `POST` | `/api/{org_id}/_search_partition_multi` | 多流搜索分区 |
| `GET` | `/api/{org_id}/{stream_names}/_around_multi` | 多流 Around 查询 |

---

## 16. HTTP/2 流式搜索

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/_search_stream` | HTTP/2 流式搜索 |
| `POST` | `/api/{org_id}/_values_stream` | HTTP/2 流式 Values |

---

## 17. 保存视图（Saved Views）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/savedviews` | 列出视图 |
| `POST` | `/api/{org_id}/savedviews` | 创建视图 |
| `GET` | `/api/{org_id}/savedviews/{view_id}` | 获取视图 |
| `PUT` | `/api/{org_id}/savedviews/{view_id}` | 更新视图 |
| `DELETE` | `/api/{org_id}/savedviews/{view_id}` | 删除视图 |

---

## 18. 函数（Functions）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/functions` | 列出函数 |
| `POST` | `/api/{org_id}/functions` | 创建函数 |
| `POST` | `/api/{org_id}/functions/test` | 测试函数 |
| `DELETE` | `/api/{org_id}/functions/bulk` | 批量删除函数 |
| `GET` | `/api/{org_id}/functions/{name}` | 获取函数/列出管道依赖 |
| `PUT` | `/api/{org_id}/functions/{name}` | 更新函数 |
| `DELETE` | `/api/{org_id}/functions/{name}` | 删除函数 |

---

## 19. 仪表盘（Dashboards）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/dashboards` | 列出仪表盘 |
| `POST` | `/api/{org_id}/dashboards` | 创建仪表盘 |
| `GET` | `/api/{org_id}/dashboards/{dashboard_id}` | 获取仪表盘 |
| `PUT` | `/api/{org_id}/dashboards/{dashboard_id}` | 更新仪表盘 |
| `DELETE` | `/api/{org_id}/dashboards/{dashboard_id}` | 删除仪表盘 |
| `DELETE` | `/api/{org_id}/dashboards/bulk` | 批量删除仪表盘 |
| `PUT` | `/api/{org_id}/folders/dashboards/{dashboard_id}` | 移动仪表盘到文件夹 |
| `PATCH` | `/api/{org_id}/dashboards/move` | 批量移动仪表盘 |
| `POST` | `/api/{org_id}/dashboards/{dashboard_id}/panels` | 添加面板 |
| `PUT` | `/api/{org_id}/dashboards/{dashboard_id}/panels/{panel_id}` | 更新面板 |
| `DELETE` | `/api/{org_id}/dashboards/{dashboard_id}/panels/{panel_id}` | 删除面板 |

---

## 20. 报告（Reports）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/reports` | 列出报告 |
| `POST` | `/api/{org_id}/reports` | 创建报告 |
| `GET` | `/api/{org_id}/reports/{name}` | 获取报告 |
| `PUT` | `/api/{org_id}/reports/{name}` | 更新报告 |
| `DELETE` | `/api/{org_id}/reports/{name}` | 删除报告 |
| `DELETE` | `/api/{org_id}/reports/bulk` | 批量删除报告 |
| `PUT` | `/api/{org_id}/reports/{name}/enable` | 启用/禁用报告 |
| `PUT` | `/api/{org_id}/reports/{name}/trigger` | 触发报告 |

---

## 21. 定时注解（Timed Annotations）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/dashboards/{dashboard_id}/annotations` | 获取注解 |
| `POST` | `/api/{org_id}/dashboards/{dashboard_id}/annotations` | 创建注解 |
| `DELETE` | `/api/{org_id}/dashboards/{dashboard_id}/annotations` | 删除注解 |
| `PUT` | `/api/{org_id}/dashboards/{dashboard_id}/annotations/{id}` | 更新注解 |
| `DELETE` | `/api/{org_id}/dashboards/{dashboard_id}/annotations/panels/{id}` | 删除面板注解 |

---

## 22. 报告 v2

前缀: `/api/v2/{org_id}/reports`

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/v2/{org_id}/reports` | 列出报告 |
| `POST` | `/api/v2/{org_id}/reports` | 创建报告 |
| `DELETE` | `/api/v2/{org_id}/reports/bulk` | 批量删除 |
| `PATCH` | `/api/v2/{org_id}/reports/move` | 移动报告 |
| `GET` | `/api/v2/{org_id}/reports/{report_id}` | 获取报告 |
| `PUT` | `/api/v2/{org_id}/reports/{report_id}` | 更新报告 |
| `DELETE` | `/api/v2/{org_id}/reports/{report_id}` | 删除报告 |
| `PATCH` | `/api/v2/{org_id}/reports/{report_id}/enable` | 启用/禁用 |
| `PUT` | `/api/v2/{org_id}/reports/{report_id}/trigger` | 触发报告 |

---

## 23. 文件夹 v2（Folders）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/v2/{org_id}/folders/{folder_type}` | 列出文件夹 |
| `POST` | `/api/v2/{org_id}/folders/{folder_type}` | 创建文件夹 |
| `GET` | `/api/v2/{org_id}/folders/{folder_type}/{folder_id}` | 获取文件夹 |
| `PUT` | `/api/v2/{org_id}/folders/{folder_type}/{folder_id}` | 更新文件夹 |
| `DELETE` | `/api/v2/{org_id}/folders/{folder_type}/{folder_id}` | 删除文件夹 |
| `GET` | `/api/v2/{org_id}/folders/{folder_type}/name/{folder_name}` | 按名称获取 |

---

## 24. 告警 v2（Alerts）

前缀: `/api/v2/{org_id}/alerts`

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `.../alerts` | 列出告警 |
| `POST` | `.../alerts` | 创建告警 |
| `GET` | `.../alerts/{alert_id}` | 获取告警 |
| `PUT` | `.../alerts/{alert_id}` | 更新告警 |
| `DELETE` | `.../alerts/{alert_id}` | 删除告警 |
| `POST` | `.../alerts/{alert_id}/export` | 导出告警 |
| `DELETE` | `.../alerts/bulk` | 批量删除 |
| `PATCH` | `.../alerts/{alert_id}/enable` | 启用/禁用 |
| `POST` | `.../alerts/bulk/enable` | 批量启用/禁用 |
| `PATCH` | `.../alerts/{alert_id}/trigger` | 触发告警 |
| `PATCH` | `.../alerts/{alert_id}/retrain` | 重新训练 |
| `POST` | `.../alerts/{alert_id}/clone` | 克隆告警 |
| `POST` | `.../alerts/generate_sql` | 生成 SQL |
| `PATCH` | `.../alerts/move` | 移动告警 |
| `GET` | `.../alerts/history` | 告警历史 |
| `GET` | `.../alerts/dedup/summary` | 去重摘要 |

---

## 25. 告警事件（Incidents）

前缀: `/api/v2/{org_id}/alerts/incidents`

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `.../incidents` | 列出事件 |
| `GET` | `.../incidents/stats` | 事件统计 |
| `GET` | `.../incidents/{incident_id}` | 获取事件 |
| `POST` | `.../incidents/{incident_id}/rca` | 触发根因分析 |
| `PATCH` | `.../incidents/{incident_id}/update` | 更新事件 |
| `GET` | `.../incidents/{incident_id}/events` | 获取事件详情 |
| `POST` | `.../incidents/{incident_id}/events/comment` | 发布评论 |

---

## 26. 告警模板（Alert Templates）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/alerts/templates` | 列出模板 |
| `POST` | `/api/{org_id}/alerts/templates` | 创建模板 |
| `GET` | `/api/{org_id}/alerts/templates/system/prebuilt` | 获取系统预置模板 |
| `GET` | `/api/{org_id}/alerts/templates/{template_name}` | 获取模板 |
| `PUT` | `/api/{org_id}/alerts/templates/{template_name}` | 更新模板 |
| `DELETE` | `/api/{org_id}/alerts/templates/{template_name}` | 删除模板 |
| `DELETE` | `/api/{org_id}/alerts/templates/bulk` | 批量删除 |

---

## 27. 告警目标（Alert Destinations）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/alerts/destinations` | 列出目标 |
| `POST` | `/api/{org_id}/alerts/destinations` | 创建目标 |
| `GET` | `/api/{org_id}/alerts/destinations/prebuilt` | 列出预置目标 |
| `GET` | `/api/{org_id}/alerts/destinations/{name}` | 获取目标 |
| `PUT` | `/api/{org_id}/alerts/destinations/{name}` | 更新目标 |
| `DELETE` | `/api/{org_id}/alerts/destinations/{name}` | 删除目标 |
| `POST` | `/api/{org_id}/alerts/destinations/test` | 测试目标 |
| `DELETE` | `/api/{org_id}/alerts/destinations/bulk` | 批量删除 |

---

## 28. 去重（Deduplication）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/alerts/deduplication/config` | 获取去重配置 |
| `POST` | `/api/{org_id}/alerts/deduplication/config` | 设置去重配置 |
| `DELETE` | `/api/{org_id}/alerts/deduplication/config` | 删除去重配置 |
| `GET` | `/api/{org_id}/alerts/deduplication/semantic-groups` | 获取语义分组 |
| `PUT` | `/api/{org_id}/alerts/deduplication/semantic-groups` | 保存语义分组 |
| `POST` | `/api/{org_id}/alerts/deduplication/semantic-groups/preview-diff` | 预览 Diff |

---

## 29. KV 存储

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/kv` | 列出所有 KV |
| `GET` | `/api/{org_id}/kv/{key}` | 获取值 |
| `POST` | `/api/{org_id}/kv/{key}` | 设置值 |
| `DELETE` | `/api/{org_id}/kv/{key}` | 删除值 |

---

## 30. 富化表（Enrichment Tables）

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/enrichment_tables/{table_name}` | 上传富化表 |
| `POST` | `/api/{org_id}/enrichment_tables/{table_name}/url` | 从 URL 导入 |
| `GET` | `/api/{org_id}/enrichment_tables/status` | 获取所有状态 |

---

## 31. 权限管理（Authz/FGA）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/roles` | 列出角色 |
| `POST` | `/api/{org_id}/roles` | 创建角色 |
| `DELETE` | `/api/{org_id}/roles/bulk` | 批量删除角色 |
| `PUT` | `/api/{org_id}/roles/{role_id}` | 更新角色 |
| `DELETE` | `/api/{org_id}/roles/{role_id}` | 删除角色 |
| `GET` | `/api/{org_id}/roles/{role_id}/permissions/{resource}` | 获取角色权限 |
| `GET` | `/api/{org_id}/roles/{role_id}/users` | 获取角色用户 |
| `GET` | `/api/{org_id}/groups` | 列出分组 |
| `POST` | `/api/{org_id}/groups` | 创建分组 |
| `GET` | `/api/{org_id}/groups/{group_name}` | 获取分组详情 |
| `PUT` | `/api/{org_id}/groups/{group_name}` | 更新分组 |
| `DELETE` | `/api/{org_id}/groups/{group_name}` | 删除分组 |
| `DELETE` | `/api/{org_id}/groups/bulk` | 批量删除分组 |
| `GET` | `/api/{org_id}/resources` | 列出资源 |
| `GET` | `/api/{org_id}/users/{user_id}/roles` | 获取用户角色 |
| `GET` | `/api/{org_id}/users/{user_id}/groups` | 获取用户分组 |

---

## 32. 集群管理

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/clusters` | 列出集群 |

---

## 33. 流水线（Pipelines）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/pipelines` | 列出流水线 |
| `POST` | `/api/{org_id}/pipelines` | 创建流水线 |
| `PUT` | `/api/{org_id}/pipelines` | 更新流水线 |
| `GET` | `/api/{org_id}/pipelines/{pipeline_id}` | 获取流水线 |
| `DELETE` | `/api/{org_id}/pipelines/{pipeline_id}` | 删除流水线 |
| `DELETE` | `/api/{org_id}/pipelines/bulk` | 批量删除 |
| `PUT` | `/api/{org_id}/pipelines/{pipeline_id}/enable` | 启用/禁用 |
| `POST` | `/api/{org_id}/pipelines/bulk/enable` | 批量启用/禁用 |
| `GET` | `/api/{org_id}/pipelines/streams` | 列出关联流 |
| `GET` | `/api/{org_id}/pipelines/history` | 流水线历史 |

---

## 34. 流水线回填（Pipeline Backfills）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/pipelines/backfill` | 列出回填任务 |
| `POST` | `/api/{org_id}/pipelines/{pipeline_id}/backfill` | 创建回填任务 |
| `GET` | `/api/{org_id}/pipelines/{pipeline_id}/backfill/{job_id}` | 获取回填任务 |
| `PUT` | `/api/{org_id}/pipelines/{pipeline_id}/backfill/{job_id}` | 更新回填任务 |
| `DELETE` | `/api/{org_id}/pipelines/{pipeline_id}/backfill/{job_id}` | 删除回填任务 |
| `PUT` | `/api/{org_id}/pipelines/{pipeline_id}/backfill/{job_id}/enable` | 启用回填 |

---

## 35. 短链接

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/short` | 创建短链接 |
| `GET` | `/api/{org_id}/short/{short_id}` | 获取原始链接 |

---

## 36. 服务账号

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/service_accounts` | 列出服务账号 |
| `POST` | `/api/{org_id}/service_accounts` | 创建服务账号 |
| `DELETE` | `/api/{org_id}/service_accounts/bulk` | 批量删除 |
| `PUT` | `/api/{org_id}/service_accounts/{email_id}` | 更新服务账号 |
| `DELETE` | `/api/{org_id}/service_accounts/{email_id}` | 删除服务账号 |

---

## 37. MCP

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/mcp` | MCP GET 请求 |
| `POST` | `/api/{org_id}/mcp` | MCP POST 请求 |
| `DELETE` | `/api/{org_id}/mcp` | MCP DELETE 请求 |

---

## 38. Sourcemaps

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/sourcemaps` | 列出 Sourcemaps |
| `POST` | `/api/{org_id}/sourcemaps` | 上传 Sourcemaps |
| `DELETE` | `/api/{org_id}/sourcemaps` | 删除 Sourcemaps |
| `GET` | `/api/{org_id}/sourcemaps/values` | 列出 Values |
| `POST` | `/api/{org_id}/sourcemaps/stacktrace` | 翻译 Stacktrace |

---

## 39. AWS 集成

认证: AWS 签名认证

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/aws/{org_id}/{stream_name}/_kinesis_firehose` | Kinesis Firehose 写入 |

---

## 40. GCP 集成

认证: GCP 认证

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/gcp/{org_id}/{stream_name}/_sub` | GCP Pub/Sub 推送 |

---

## 41. RUM 集成

认证: RUM Token 认证

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/rum/v1/{org_id}/logs` | RUM 日志写入 |
| `POST` | `/rum/v1/{org_id}/replay` | Session Replay 写入 |
| `POST` | `/rum/v1/{org_id}/rum` | RUM 数据写入 |

---

## 42. 代理路由（Proxy）

认证: Proxy URL 认证

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/proxy/{org_id}/{*target_url}` | URL 代理 |

---

## Enterprise 功能

以下 API 仅在 Enterprise 版本中可用：

### 异常检测（Anomaly Detection）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/anomaly_detection` | 列出配置 |
| `POST` | `/api/{org_id}/anomaly_detection` | 创建配置 |
| `GET` | `/api/{org_id}/anomaly_detection/{config_id}` | 获取配置 |
| `PUT` | `/api/{org_id}/anomaly_detection/{config_id}` | 更新配置 |
| `DELETE` | `/api/{org_id}/anomaly_detection/{config_id}` | 删除配置 |
| `POST` | `/api/{org_id}/anomaly_detection/{config_id}/train` | 训练模型 |
| `DELETE` | `/api/{org_id}/anomaly_detection/{config_id}/train` | 取消训练 |
| `POST` | `/api/{org_id}/anomaly_detection/{config_id}/detect` | 检测异常 |
| `GET` | `/api/{org_id}/anomaly_detection/{config_id}/history` | 检测历史 |

### 搜索任务（Search Jobs）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/search_jobs` | 列出任务状态 |
| `POST` | `/api/{org_id}/search_jobs` | 提交搜索任务 |
| `GET` | `/api/{org_id}/search_jobs/{job_id}` | 获取任务状态 |
| `DELETE` | `/api/{org_id}/search_jobs/{job_id}` | 删除任务 |
| `GET` | `/api/{org_id}/search_jobs/{job_id}/result` | 获取任务结果 |
| `POST` | `/api/{org_id}/search_jobs/{job_id}/cancel` | 取消任务 |
| `POST` | `/api/{org_id}/search_jobs/{job_id}/retry` | 重试任务 |

### 查询管理（Query Manager）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/query_manager/status` | 查询状态 |
| `PUT` | `/api/{org_id}/query_manager/cancel` | 批量取消查询 |
| `DELETE` | `/api/{org_id}/query_manager/{query_id}/cancel` | 取消单个查询 |

### 搜索检查器（Search Inspector）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/search/profile` | 获取搜索 Profile |

### 密钥管理（Cipher Keys）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/cipher_keys` | 列出密钥 |
| `POST` | `/api/{org_id}/cipher_keys` | 创建密钥 |
| `DELETE` | `/api/{org_id}/cipher_keys/bulk` | 批量删除 |
| `GET` | `/api/{org_id}/cipher_keys/{key_name}` | 获取密钥 |
| `PUT` | `/api/{org_id}/cipher_keys/{key_name}` | 更新密钥 |
| `DELETE` | `/api/{org_id}/cipher_keys/{key_name}` | 删除密钥 |

### Actions

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/actions` | 列出 Actions |
| `POST` | `/api/{org_id}/actions/upload` | 上传 Action (zip) |
| `DELETE` | `/api/{org_id}/actions/bulk` | 批量删除 |
| `GET` | `/api/{org_id}/actions/{action_id}` | 获取 Action |
| `PUT` | `/api/{org_id}/actions/{action_id}` | 更新 Action |
| `DELETE` | `/api/{org_id}/actions/{action_id}` | 删除 Action |
| `GET` | `/api/{org_id}/actions/download/{action_id}` | 下载 Action |
| `GET` | `/api/{org_id}/actions/pause/{action_id}` | 暂停 Action |
| `GET` | `/api/{org_id}/actions/resume/{action_id}` | 恢复 Action |
| `POST` | `/api/{org_id}/actions/test/{action_id}` | 测试 Action |

### 限速（Rate Limits）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/ratelimit/api_modules` | API 模块列表 |
| `GET` | `/api/{org_id}/ratelimit/module_list` | 模块限速列表 |
| `GET` | `/api/{org_id}/ratelimit/role_list` | 角色限速列表 |
| `PUT` | `/api/{org_id}/ratelimit/update` | 更新限速配置 |

### AI

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/ai/chat` | AI 对话 |
| `POST` | `/api/{org_id}/ai/chat_stream` | AI 流式对话 |
| `POST` | `/api/{org_id}/ai/feedback` | AI 反馈 |
| `POST` | `/api/{org_id}/ai/confirm/{session_id}` | 确认 Action |
| `GET` | `/api/{org_id}/ai/toolsets` | 列出工具集 |
| `POST` | `/api/{org_id}/ai/toolsets` | 创建工具集 |
| `GET` | `/api/{org_id}/ai/toolsets/{id}` | 获取工具集 |
| `PUT` | `/api/{org_id}/ai/toolsets/{id}` | 更新工具集 |
| `DELETE` | `/api/{org_id}/ai/toolsets/{id}` | 删除工具集 |

### 评估模板（Evaluation Templates）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/eval_templates` | 列出模板 |
| `POST` | `/api/{org_id}/eval_templates` | 创建模板 |
| `GET` | `/api/{org_id}/eval_templates/{template_id}` | 获取模板 |
| `PUT` | `/api/{org_id}/eval_templates/{template_id}` | 更新模板 |
| `DELETE` | `/api/{org_id}/eval_templates/{template_id}` | 删除模板 |
| `GET` | `/api/{org_id}/eval_templates/{template_id}/stats` | 获取统计 |

### 正则模式（RE Patterns）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/re_patterns` | 列出模式 |
| `POST` | `/api/{org_id}/re_patterns` | 创建模式 |
| `GET` | `/api/{org_id}/re_patterns/{pattern_id}` | 获取模式 |
| `PUT` | `/api/{org_id}/re_patterns/{pattern_id}` | 更新模式 |
| `DELETE` | `/api/{org_id}/re_patterns/{pattern_id}` | 删除模式 |
| `GET` | `/api/{org_id}/re_patterns/built-in` | 获取内置模式 |
| `POST` | `/api/{org_id}/re_patterns/test` | 测试模式 |
| `DELETE` | `/api/{org_id}/re_patterns/bulk` | 批量删除 |

### 域名管理（Domain Management）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/domain_management` | 获取域名配置 |
| `PUT` | `/api/{org_id}/domain_management` | 设置域名配置 |

### License

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/license` | 获取 License 信息 |
| `POST` | `/api/license` | 存储 License |

### 拓扑（Topology）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/traces/service_graph/topology/current` | 当前拓扑 |
| `GET` | `/api/{org_id}/traces/service_graph/edge/history` | 边历史 |

### 模式提取（Patterns）

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/{org_id}/streams/{stream_name}/patterns/extract` | 提取模式 |

### 服务流（Service Streams）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/{org_id}/service_streams` | 列出服务 |
| `GET` | `/api/{org_id}/service_streams/_analytics` | 维度分析 |
| `POST` | `/api/{org_id}/service_streams/_correlate` | 关联流 |
| `GET` | `/api/{org_id}/service_streams/config/identity` | 获取身份配置 |
| `PUT` | `/api/{org_id}/service_streams/config/identity` | 保存身份配置 |
| `DELETE` | `/api/{org_id}/service_streams/_reset` | 重置服务 |

---

## 通用说明

### 认证方式
- **标准 API**: Bearer Token / Basic Auth（通过 `auth_middleware`）
- **AWS 集成**: AWS 签名认证（通过 `aws_auth_middleware`）
- **GCP 集成**: GCP 认证（通过 `gcp_auth_middleware`）
- **RUM 集成**: RUM Token 认证（通过 `rum_auth_middleware`）

### 路径参数
- `{org_id}`: 组织 ID
- `{stream_name}`: 数据流名称
- `{email_id}`: 用户邮箱

### 请求体限制
- 由 `ZO_REQ_PAYLOAD_LIMIT` 配置控制

### 中间件链（执行顺序）
1. Snappy 预处理解压
2. 标准解压 (gzip/deflate/brotli)
3. CORS
4. Server Header
5. Auth 认证
6. Audit 审计
7. Blocked Orgs 检查

### Swagger 文档
- 当 `swagger_enabled=true` 时可访问 `/swagger` 或 `/docs`
