# PR: Fix PostgreSQL Dashboard — PromQL label mismatch causing "No Data" panels

## Summary

Fix two panels in the PostgreSQL Metrics dashboard that display **"No Data"** due to PromQL binary operation label mismatch. The `postgresql_backends` metric carries a `postgresql_database_name` label, while `postgresql_connection_max` does not. When PromQL performs arithmetic between two vectors, it requires matching label sets on both sides — mismatched labels result in empty results.

Additionally, the `Connection Utilization %` panel legend template `{postgresql_database_name}` renders as a raw string because `sum()` aggregation removes all labels, making the template variable unresolvable.

---

## Problem

### Root Cause: Label Mismatch in PromQL Binary Operations

The OpenTelemetry PostgreSQL receiver emits metrics with different label sets:

| Metric | Labels |
|--------|--------|
| `postgresql_backends` | `postgresql_database_name`, `service_instance_id`, `instrumentation_library_name`, ... |
| `postgresql_connection_max` | `service_instance_id`, `instrumentation_library_name`, ... (**no** `postgresql_database_name`) |

When performing division `postgresql_backends / postgresql_connection_max`, PromQL requires **exact label matching** between left and right operands. Since `postgresql_backends` has an extra `postgresql_database_name` label, no vector elements match → **result is empty ("No Data")**.

### Affected Panels

#### 1. Connection Utilization %
- **Symptom**: Shows "No Data"
- **Secondary issue**: Legend displays raw template `{postgresql_database_name}` instead of a meaningful name

#### 2. Database Health Score
- **Symptom**: Shows "No Data"
- **Cause**: Same label mismatch issue in the compound formula where `postgresql_backends`, `postgresql_connection_max`, `postgresql_blks_hit`, `postgresql_blks_read`, `postgresql_commits`, and `postgresql_rollbacks` all have different label sets

---

## Fix

Wrap all metrics in `sum()` to aggregate away label dimensions before performing arithmetic. This ensures both sides of binary operations have matching (empty) label sets.

### Panel: Connection Utilization %

**Before:**
```promql
(postgresql_backends / postgresql_connection_max)
```

**After:**
```promql
(sum(postgresql_backends) / sum(postgresql_connection_max)) * 100
```

**Legend fix:** Changed from `{postgresql_database_name}` → `Utilization %`
- Reason: `sum()` removes all labels, so template variables cannot be resolved and render as raw strings.

### Panel: Database Health Score

**Before:**
```promql
((rate(postgresql_blks_hit[5m]) / (rate(postgresql_blks_hit[5m]) + rate(postgresql_blks_read[5m]))) * 0.40)
+ ((rate(postgresql_commits[5m]) / (rate(postgresql_commits[5m]) + rate(postgresql_rollbacks[5m]))) * 0.30)
+ (((postgresql_connection_max - postgresql_backends) / postgresql_connection_max) * 0.30)
```

**After:**
```promql
((sum(rate(postgresql_blks_hit[5m])) / (sum(rate(postgresql_blks_hit[5m])) + sum(rate(postgresql_blks_read[5m])))) * 0.40)
+ ((sum(rate(postgresql_commits[5m])) / (sum(rate(postgresql_commits[5m])) + sum(rate(postgresql_rollbacks[5m])))) * 0.30)
+ (((sum(postgresql_connection_max) - sum(postgresql_backends)) / sum(postgresql_connection_max)) * 0.30)
```

---

## Verification

Tested against OpenObserve with otelcol-contrib v0.151.0 collecting from PostgreSQL 18.3:

```bash
# Connection Utilization % — returns 3 (i.e., 3%)
$ curl -s "http://localhost:5080/api/default/prometheus/api/v1/query" \
  --data-urlencode "query=(sum(postgresql_backends) / sum(postgresql_connection_max)) * 100"
{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1778147712,"3"]}]}}

# Database Health Score — returns ~0.99
$ curl -s "http://localhost:5080/api/default/prometheus/api/v1/query" \
  --data-urlencode "query=((sum(rate(postgresql_blks_hit[5m])) / ...) * 0.40) + ..."
{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1778147729,"0.991"]}]}}
```

---

## Environment

- **OpenObserve**: latest (running on localhost:5080)
- **otelcol-contrib**: v0.151.0
- **PostgreSQL**: 18.3
- **OS**: Ubuntu 22.04 (Linux x86_64)

---

## Files Changed

- `PostgreSQL Metrics/PostgreSQL.dashboard.json`
  - Fixed `Connection Utilization %` panel query and legend
  - Fixed `Database Health Score` panel query
