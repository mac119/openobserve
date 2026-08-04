# Design Proposal: Tiered Storage (S3 + Local Storage)

## Prior Art Research

We investigated how two major analytical engines handle tiered/cached storage for S3-backed workloads:

### ClickHouse — Storage Policy + TTL MOVE

ClickHouse implements tiered storage natively through its **Storage Policy** mechanism:

```xml
<storage_configuration>
  <disks>
    <local_ssd>
      <type>local</type>
      <path>/var/lib/clickhouse/data/</path>
    </local_ssd>
    <s3_cold>
      <type>s3</type>
      <endpoint>https://bucket.s3.amazonaws.com/</endpoint>
      <metadata_path>/var/lib/clickhouse/disks/s3/</metadata_path>
    </s3_cold>
  </disks>
  <policies>
    <tiered>
      <volumes>
        <hot><disk>local_ssd</disk></hot>
        <cold><disk>s3_cold</disk></cold>
      </volumes>
      <move_factor>0.2</move_factor>
    </tiered>
  </policies>
</storage_configuration>
```

Data parts are written to the `hot` volume first. Migration to `cold` (S3) is triggered by **three mechanisms**:

| Trigger | How it works |
|---------|--------------|
| **TTL MOVE** | `TTL event_date + INTERVAL 7 DAY TO VOLUME 'cold'` — automatic age-based promotion |
| **move_factor** | When hot volume free space drops below threshold (default 10%), oldest parts migrate automatically |
| **Manual SQL** | `ALTER TABLE ... MOVE PARTITION '2025-01' TO DISK 's3_cold'` — operator-initiated |

Key design decisions:
- **Metadata stays local** — S3 disk keeps a local `metadata_path` with lightweight index/mark files (few KB per part), so query planning never hits S3
- **Async & non-blocking** — migration happens in background; reads/writes continue uninterrupted
- **Per-table granularity** — each table can have its own storage policy and TTL rules
- **S3 cache layer** — a `<type>cache</type>` disk wraps the S3 disk to accelerate repeated cold reads on local SSD

### DuckDB — diskcache Extension (Transparent Filesystem Interception)

DuckDB takes the **opposite direction**: data lives on S3, and reads are transparently cached to local SSD.

The [duckdb-diskcache](https://github.com/peterboncz/duckdb-diskcache) extension implements a **4-level cache hierarchy**:

```
L1: WriteBuffer (shared_ptr, RAM)     ~10ns
L2: blobfile_memcache (RAM)           ~100ns
L3: .diskcache/ directory (SSD)       ~100μs    ← core layer
L4: Remote S3/HTTP                    ~10-100ms
```

Architecture:
- **Non-invasive** — wraps the filesystem layer (`DiskcacheFileSystemWrapper`), no query changes needed
- **Range-granular** — caches byte ranges, not whole files; tracks per-range access counts for LRU
- **Partition-parallel I/O** — write jobs distributed via `file_id % nr_io_threads` to avoid lock contention
- **Immutable-file safe** — default "safe mode" only caches Parquet files accessed through Iceberg/Delta (guaranteed immutable), eliminating cache invalidation concerns
- **Persistent** — SSD cache survives DuckDB restart; no re-download needed

---

## OpenObserve Current Architecture

OpenObserve's existing data flow:

```
Ingest → MemTable → WAL → Immutable → Parquet (local)
  → file_push job (every 10s): merge small parquets → upload to S3 → delete local
  → Query: file_list lookup → memory_cache → disk_cache → S3 download
```

**The problem**: Parquet files are uploaded to S3 immediately (within ~600s of ingestion), then the *same files* are downloaded back from S3 on the very next query (cache miss). For the most common query pattern — recent data (last 1h–24h) — this creates unnecessary S3 PUT + GET costs and adds network latency.

OpenObserve's key structural advantage: **all Parquet files are write-once/immutable** (never modified after creation). This means:
- No cache invalidation complexity (unlike mutable data systems)
- Hot-tier files can be read safely during promotion (no dirty-read risk)
- Existing `SEARCHING_FILES` lock already prevents deletion of in-use files

---

## Proposed Design for OpenObserve

Combining lessons from both ClickHouse (write-path tiering) and DuckDB (read-path caching), we propose:

### Core Concept

```
Current:  WAL → parquet → merge → S3 upload → delete local → (query cache miss → S3 GET)
Proposed: WAL → parquet → merge → LOCAL HOT TIER → [trigger] → S3 upload → delete local
                                       ↑                            ↑
                                  query: direct local read     query: existing cache → S3
```

Hot-tier files stay on fast local disk. Queries targeting recent data read directly from local storage — zero S3 cost, zero network latency. When files age out (or disk pressure occurs), they promote to S3 and the existing disk/memory cache layer handles cold reads.

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `ZO_TIERED_STORAGE_ENABLED` | `false` | Enable tiered storage (backward compatible) |
| `ZO_TIERED_STORAGE_TTL` | `86400` (24h) | Seconds to keep files in hot tier |
| `ZO_TIERED_STORAGE_MAX_SIZE` | `0` (unlimited) | Max hot tier size in MB; triggers early promotion |
| `ZO_TIERED_STORAGE_DIR` | `{data_dir}/hot/` | Hot tier directory (recommend fast NVMe) |

Per-stream override via Stream Settings:
```json
{ "tiered_storage_ttl": 604800 }  // 7 days for critical streams
```

### Three Promotion Triggers

| # | Trigger | Condition | Inspired by |
|---|---------|-----------|-------------|
| 1 | **Time-based (TTL)** | `file.max_ts < now - TTL` | ClickHouse `TTL ... TO VOLUME` |
| 2 | **Space-pressure** | hot tier usage exceeds max_size | ClickHouse `move_factor` |
| 3 | **Manual API** | `POST /api/{org}/streams/{stream}/_promote` | ClickHouse `MOVE PARTITION TO DISK` |

Priority: Space-pressure > Manual > TTL

### File Metadata Extension

Add a `tier` field to file_list (SQLite/PostgreSQL):

```sql
ALTER TABLE file_list ADD COLUMN tier SMALLINT NOT NULL DEFAULT 1;
-- 0 = Hot (local), 1 = Cold (S3) — default Cold for backward compatibility
```

### Modified Ingestion Path

In `file_push` job (`src/job/files/parquet.rs`), when tiered storage is enabled:
- After merging small parquets, write the result to `ZO_TIERED_STORAGE_DIR` (local) instead of uploading to S3
- Register in file_list with `tier = Hot`

### Promotion Job

New background task (`src/job/files/tiered_promote.rs`):
- Runs every 60s (configurable)
- Scans file_list for Hot files past TTL or exceeding space limit
- Skips files in `SEARCHING_FILES` (currently being queried)
- Uploads to S3 → updates tier to Cold → deletes local file
- Parallelizes via batch + concurrent uploads

### Query Path

Modify `cache_files()` in `src/service/search/grpc/storage.rs`:
- Hot-tier files → direct local read (treat as already cached)
- Cold-tier files → existing logic (memory cache → disk cache → S3)

### Cluster Mode

- Each **Ingester** maintains its own hot tier directory
- **file_list** (PostgreSQL) tracks which node owns each Hot file
- **Querier** routes hot-tier reads to owning Ingester via existing gRPC search endpoint (same path as current WAL queries)
- **Compactor** skips Hot-tier files; compaction only runs on Cold data

### Edge Cases

| Scenario | Handling |
|----------|----------|
| Node crash | Hot files survive on disk; startup recovery re-registers them in file_list |
| Disk full | Space-pressure promotion immediately evacuates oldest files |
| Query during promotion | `SEARCHING_FILES` lock prevents deletion of in-use files |
| S3 unreachable | Promotion retries with backoff; hot files remain queryable |

---

## Summary: Why This Design Works Well for OpenObserve

| Property | Benefit |
|----------|---------|
| Files are immutable (write-once) | No cache invalidation needed (borrowed from DuckDB's insight) |
| Existing disk cache handles cold reads | No new cache layer to build |
| `SEARCHING_FILES` lock exists | Safe concurrent access during promotion |
| file_list already tracks all files | Just add one column for tier |
| Ingester gRPC search exists | Cluster-mode hot reads work out of the box |

The design is **default-off** and fully backward compatible. Enabling it only changes where merged Parquet files land initially (local hot dir vs immediate S3 upload) and adds a background promotion job.

Happy to discuss further or start with a PoC PR for Phase 1 (config + metadata extension).
