# RFC: Tiered Storage (S3 + Local Storage) — Design Proposal for #10214

## Summary

Introduce a **two-tier storage architecture** where recently-ingested Parquet files remain on fast local disk (hot tier) for a configurable TTL before being promoted to S3/object storage (cold tier). This reduces S3 PUT/GET costs and improves query latency for recent data — the most frequently queried time range.

## Motivation

Currently, OpenObserve's `file_push` job uploads Parquet files to S3 **immediately** after WAL compaction (default: every 10s scan, upload when file age > 600s or size > 256MB). This means:

1. **High S3 request costs** — Every ingested file triggers PUT operations; every recent-data query triggers GET operations (or cache miss downloads)
2. **Unnecessary latency** — Most queries target recent data (last 1h–24h), yet those files already left local disk
3. **Cache redundancy** — The disk cache (`ZO_DISK_CACHE`) downloads the same files back from S3 that were just uploaded moments ago

A tiered approach keeps hot data local (zero S3 cost, zero network latency) while cold data lives cheaply on S3.

## Design

### Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                     Query Path                           │
│                                                         │
│  file_list.query() → check tier:                        │
│    HOT  → read directly from local_data_dir             │
│    COLD → memory_cache → disk_cache → S3 download       │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│                   Ingestion Path                          │
│                                                         │
│  WAL → MemTable → Immutable → Parquet (local)           │
│    → file_push merge → LOCAL HOT TIER (new!)            │
│       (stays on local disk for ZO_TIERED_STORAGE_TTL)   │
│                                                         │
│  promotion_job (new):                                   │
│    scan hot files where max_ts < now - TTL              │
│    → storage::put() to S3                               │
│    → update file_list tier = COLD                       │
│    → delete local file                                  │
└─────────────────────────────────────────────────────────┘
```

### Data Flow (Modified)

```
Current:  WAL → parquet → merge → S3 upload → delete local → (cache miss → download from S3)
Proposed: WAL → parquet → merge → LOCAL HOT → [TTL expires] → S3 upload → delete local
                                      ↑                            ↑
                                query: direct read           query: cache/S3
```

### Key Components

#### 1. Configuration (New Environment Variables)

| Variable | Default | Description |
|----------|---------|-------------|
| `ZO_TIERED_STORAGE_ENABLED` | `false` | Enable tiered storage |
| `ZO_TIERED_STORAGE_TTL` | `86400` (24h) | Seconds to keep files in hot tier before promoting to S3 |
| `ZO_TIERED_STORAGE_MAX_SIZE` | `0` (unlimited) | Max local hot tier size in MB; triggers early promotion when exceeded |
| `ZO_TIERED_STORAGE_DIR` | `{data_dir}/hot/` | Directory for hot tier files (can be fast NVMe) |
| `ZO_TIERED_STORAGE_PROMOTE_INTERVAL` | `60` | Seconds between promotion job scans |
| `ZO_TIERED_STORAGE_PROMOTE_BATCH` | `100` | Max files to promote per batch |

#### 2. File Metadata Extension

**File: `src/infra/src/file_list/mod.rs`**

Add a `storage_tier` field to file metadata:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum StorageTier {
    Hot,   // file exists on local disk (hot tier dir)
    Cold,  // file exists on remote object storage (S3)
}

// Extend FileMeta or FileKey with:
pub struct FileKey {
    pub key: String,
    pub meta: FileMeta,
    pub tier: StorageTier,  // NEW
}
```

**Schema migration**: Add `tier` column to `file_list` table (SQLite/PostgreSQL):
```sql
ALTER TABLE file_list ADD COLUMN tier SMALLINT NOT NULL DEFAULT 1;
-- 0 = Hot, 1 = Cold (default Cold for backward compatibility)
```

#### 3. Modified File Push Job

**File: `src/job/files/parquet.rs`**

Current `move_files()` uploads to S3 immediately. Modified behavior:

```rust
async fn move_files(...) {
    // ... existing merge logic ...
    
    if cfg.common.tiered_storage_enabled {
        // Write merged parquet to hot tier directory
        let hot_path = format!("{}/{}", cfg.common.tiered_storage_dir, new_file_key);
        tokio::fs::write(&hot_path, &buf).await?;
        
        // Register in file_list with tier = Hot
        db::file_list::set(&account, &new_file_name, Some(new_file_meta), false)
            .await?;
        db::file_list::set_tier(&new_file_name, StorageTier::Hot).await?;
    } else {
        // Original behavior: upload to S3 immediately
        storage::put(&account, &new_file_key, buf).await?;
        db::file_list::set(&account, &new_file_name, Some(new_file_meta), false)
            .await?;
    }
    
    // Delete WAL source files (same as before)
    // ...
}
```

#### 4. Promotion Job (New)

**New file: `src/job/files/tiered_promote.rs`**

```rust
/// Background job that promotes hot-tier files to cold tier (S3)
/// Runs every ZO_TIERED_STORAGE_PROMOTE_INTERVAL seconds
pub async fn run() -> Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(cfg.tiered_storage_promote_interval)).await;
        
        if let Err(e) = promote_expired_files().await {
            log::error!("tiered storage promotion failed: {e}");
        }
    }
}

async fn promote_expired_files() -> Result<()> {
    let cutoff = Utc::now().timestamp_micros() 
        - cfg.tiered_storage_ttl * 1_000_000;
    
    // Query files: tier=Hot AND max_ts < cutoff
    let files = db::file_list::query_hot_expired(cutoff, cfg.promote_batch).await?;
    
    for file in files {
        // Skip if file is currently being read by a query
        if wal::SEARCHING_FILES.contains(&file.key) {
            continue;
        }
        
        // Read from hot tier
        let hot_path = format!("{}/{}", cfg.tiered_storage_dir, file.key);
        let data = tokio::fs::read(&hot_path).await?;
        
        // Upload to S3
        let account = storage::get_account(&file.key);
        storage::put(&account, &file.key, data.into()).await?;
        
        // Update tier metadata
        db::file_list::set_tier(&file.key, StorageTier::Cold).await?;
        
        // Delete local hot file
        tokio::fs::remove_file(&hot_path).await?;
    }
    
    Ok(())
}
```

**Space-pressure promotion**: If `ZO_TIERED_STORAGE_MAX_SIZE > 0` and current hot tier usage exceeds the limit, promote oldest files regardless of TTL.

#### 5. Query Path Modification

**File: `src/service/search/grpc/storage.rs`**

Modify `cache_files()` to recognize hot-tier files:

```rust
async fn cache_files(files: &[FileKey], ...) -> Result<CacheResult> {
    let mut cached_files = Vec::new();
    let mut remote_files = Vec::new();
    
    for file in files {
        match file.tier {
            StorageTier::Hot => {
                // File is on local hot tier — treat as "cached" with local path
                let local_path = format!("{}/{}", cfg.tiered_storage_dir, file.key);
                cached_files.push(CachedFile::Local(local_path));
            }
            StorageTier::Cold => {
                // Existing logic: check memory cache → disk cache → S3
                if memory::exist(&file.key) {
                    cached_files.push(CachedFile::MemoryCached(file.key));
                } else if disk::exist(&file.key) {
                    cached_files.push(CachedFile::DiskCached(file.key));
                } else {
                    remote_files.push(file);
                }
            }
        }
    }
    // ... download remote_files in background (existing logic)
}
```

#### 6. Startup Recovery

On node restart, scan the hot tier directory and reconcile with file_list metadata:
- Files in hot dir but not in file_list → register them (crash recovery)
- Files in file_list as Hot but missing from disk → re-mark as needing re-ingestion or mark as lost

### Backward Compatibility

- **Default off**: `ZO_TIERED_STORAGE_ENABLED=false` preserves current behavior (immediate S3 upload)
- **Migration**: Existing deployments enabling tiered storage for the first time — all existing files remain Cold; only new ingested data goes to Hot tier
- **Cluster mode**: Each Ingester maintains its own hot tier; promotion job runs on each Ingester independently. Queriers can identify Hot files via file_list metadata and route reads to the appropriate Ingester via gRPC.

### Cluster Mode Considerations

In HA cluster mode:
- **Ingester** nodes maintain their own hot tier directories
- **file_list** (PostgreSQL) tracks which Ingester node holds each Hot file via existing `node_id` field
- **Querier** nodes, when encountering a Hot-tier file, either:
  - (a) Route the read via gRPC to the owning Ingester (similar to current WAL query path), OR
  - (b) The Ingester's existing `/grpc/search` endpoint already handles local files — no change needed
- **Compactor** skips Hot-tier files (only operates on Cold-tier data)

### Edge Cases & Safety

| Scenario | Handling |
|----------|----------|
| Node crash before promotion | Hot files survive on disk; startup recovery re-registers them |
| Disk full (hot tier) | Space-pressure promotion kicks in; promote oldest files immediately |
| Query during promotion | `SEARCHING_FILES` lock prevents deletion of in-use files |
| Network partition (can't reach S3) | Promotion retries with backoff; hot files remain readable |
| Multiple Ingesters with same stream | Each Ingester has independent hot tier; file_list deduplication on merge |

### Metrics & Observability

Expose new metrics:
- `openobserve_tiered_hot_files_total` — current hot tier file count
- `openobserve_tiered_hot_bytes_total` — current hot tier disk usage
- `openobserve_tiered_promotions_total` — files promoted to cold tier
- `openobserve_tiered_promotion_latency_seconds` — promotion duration histogram

### Implementation Plan

| Phase | Scope | Effort |
|-------|-------|--------|
| Phase 1 | Config + file_list tier field + schema migration | 1-2 days |
| Phase 2 | Modify file_push to write to hot tier | 2-3 days |
| Phase 3 | Promotion job (time-based + space-pressure) | 2-3 days |
| Phase 4 | Query path: read hot-tier files directly | 1-2 days |
| Phase 5 | Startup recovery + cluster mode routing | 2-3 days |
| Phase 6 | Metrics, docs, e2e tests | 1-2 days |

**Total estimate: ~10-15 days**

### Per-Stream TTL Override

Inspired by ClickHouse's per-table TTL rules, we support per-stream hot tier TTL configuration:

```
# Global default (env var)
ZO_TIERED_STORAGE_TTL=86400   # 24h

# Per-stream override via Stream Settings API/UI:
PUT /api/{org}/streams/{stream}/settings
{
  "tiered_storage_ttl": 604800   // 7 days — critical stream stays hot longer
}
```

**Resolution order**: per-stream setting > global env var > default (24h)

This allows operators to keep high-value streams (e.g., security logs, SLO metrics) in the hot tier longer while aggressively promoting low-value streams (e.g., debug logs) to S3 sooner.

### Manual Promotion API

Inspired by ClickHouse's `ALTER TABLE ... MOVE PARTITION TO DISK`, provide an explicit promotion endpoint for operational control:

```
POST /api/{org}/streams/{stream}/_promote
{
  "before": "2026-07-01T00:00:00Z",   // promote all hot files with max_ts before this time
  "dry_run": false                      // set true to preview files without moving
}

Response:
{
  "promoted_files": 42,
  "promoted_bytes": 1073741824,
  "duration_ms": 3200
}
```

Use cases:
- Emergency disk space reclamation
- Pre-migration before node decommission
- Testing promotion logic in production

### Three Promotion Triggers (Complete)

| # | Trigger | Condition | Analogy |
|---|---------|-----------|---------|
| 1 | **Time-based (TTL)** | `file.max_ts < now - tiered_storage_ttl` | ClickHouse `TTL ... TO VOLUME 'cold'` |
| 2 | **Space-pressure** | hot tier usage > `ZO_TIERED_STORAGE_MAX_SIZE` | ClickHouse `move_factor` |
| 3 | **Manual API** | Operator calls `/_promote` endpoint | ClickHouse `MOVE PARTITION TO DISK` |

Priority: Space-pressure (immediate) > Manual API (on-demand) > TTL (background)

---

## Prior Art Comparison

| Dimension | ClickHouse | DuckDB (diskcache) | OpenObserve (this proposal) |
|-----------|-----------|-------------------|---------------------------|
| **Direction** | Write hot → auto-sink cold | Read cold → auto-cache hot | **Both** (write-hot + read-cache) |
| **Trigger** | TTL + space + manual SQL | On-read (automatic) | TTL + space + manual API |
| **Granularity** | Data Part (~100s MB) | Byte Range | Parquet file (~10s MB) |
| **Metadata** | Local metadata_path (few KB) | LRU entry table | file_list + `tier` column |
| **Query transparent** | ✅ Same table | ✅ Same SQL | ✅ Same API |
| **Migration non-blocking** | ✅ Async background | N/A | ✅ `SEARCHING_FILES` lock |
| **Per-stream/table policy** | ✅ Per-table TTL | ❌ Global only | ✅ Per-stream TTL |
| **Cache for cold reads** | ✅ `<type>cache</type>` disk | ✅ L1-L4 hierarchy | ✅ Existing disk/memory cache |

### Key Lessons Borrowed

**From ClickHouse:**
- Three-trigger promotion model (TTL + space + manual) — complete operational coverage
- Per-table TTL override — flexibility for mixed workloads
- Metadata stays local, only data moves — minimal query impact
- `move_factor` as a safety valve — prevents disk-full emergencies

**From DuckDB diskcache:**
- Immutable files are safe to cache indefinitely (no staleness risk) — OpenObserve Parquet files are append-only/immutable, perfect fit
- Partition-parallel I/O (`file_id % nr_io_threads`) — our promotion job should parallelize similarly
- Persistent cache survives restart — our hot tier files naturally survive restart (they're just local files)

**OpenObserve's unique advantage:**
- Files are **write-once, never modified** — no cache invalidation complexity
- Existing `SEARCHING_FILES` lock provides safe concurrent access during promotion
- Existing disk cache layer handles cold-tier read acceleration — no new cache layer needed

---

### Open Questions

1. Should hot-tier files participate in **compaction**? (Proposal: No — compact only after promotion to cold tier, to avoid I/O amplification on local disk)
2. For cluster mode, should hot-tier queries go through the Ingester's gRPC endpoint (reuse WAL query path) or should we add a dedicated "hot tier read" RPC?
3. Should we support **per-stream promotion schedule** (e.g., cron-like) in addition to TTL-based?

---

## References

- Current file push implementation: `src/job/files/parquet.rs`
- Disk cache implementation: `src/infra/src/cache/file_data/disk.rs`
- Storage abstraction: `src/infra/src/storage/mod.rs`
- File list metadata: `src/infra/src/file_list/mod.rs`
- WAL searching lock: `src/common/infra/wal.rs` (`SEARCHING_FILES`)
- ClickHouse Tiered Storage: [Storage Policies](https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/mergetree#storage_policies), [TTL MOVE](https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/mergetree#table_engine-mergetree-ttl)
- DuckDB diskcache: [peterboncz/duckdb-diskcache](https://github.com/peterboncz/duckdb-diskcache) — transparent filesystem interception + 4-level cache hierarchy
- Grafana Loki (BoltDB Shipper), Cortex (Store Gateway), Thanos (Tiered Compaction)
