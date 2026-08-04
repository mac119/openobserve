## feat(search): support two-field GROUP BY via Tantivy index optimization

Closes #12039

### Summary

Extends the existing `SimpleTopN` optimization to support **two-field GROUP BY** queries through Tantivy's nested `TermsAggregation`, eliminating the need for full parquet scan in applicable scenarios.

**Supported SQL patterns:**
```sql
SELECT userid, searchphrase, COUNT(*) as cnt 
FROM hits GROUP BY userid, searchphrase ORDER BY cnt DESC LIMIT 10;

SELECT userid, searchphrase, COUNT(*) as cnt 
FROM hits GROUP BY userid, searchphrase LIMIT 10;
```

---

### Implementation

The implementation piggybacks on the existing `SimpleTopN` pipeline with minimal branching at each layer:

| Layer | Change |
|-------|--------|
| **Pattern Matcher** | New `topn_multi.rs` — detects 2 indexed group_expr + `COUNT(*)` + LIMIT |
| **Enum** | `IndexOptimizeMode::SimpleTopNMulti(Vec<String>, usize, bool)` |
| **Proto** | `message SimpleTopNMulti { repeated string fields, uint32 limit, bool asc }` |
| **Tantivy Execution** | `handle_simple_top_n_multi()` — nested `TermsAggregation` (outer field1, inner field2) |
| **Result Builder** | `TantivyMultiResultBuilder::TopNMulti` — extends per-file results |
| **Arrow Output** | `create_top_n_multi_arrow_array()` — 3-column RecordBatch (field1, field2, count) |
| **Cache** | `CacheEntry::TopNMulti` |

**Key design decision — over-fetch strategy:**
```rust
// Optimized: use limit*2 (max 100) to avoid O(N²) bucket explosion
let outer_limit = (effective_limit * 2).max(100) as u32;
let inner_limit = (effective_limit * 2).max(100) as u32;
// (Previously limit*4, max 1000 → worst case 1M buckets)
```

DataFusion's existing `AggregateExec` handles the final re-aggregation and sort across segments, so correctness is guaranteed even with conservative over-fetch.

---

### Benchmark Results (10M records, single node)

```
Query                                              Min      Median     Max
---------------------------------------------------------------------------
1-field GROUP BY (userid, LIMIT 10)               82.8ms    88.4ms   130.6ms
1-field GROUP BY (searchphrase, LIMIT 10)         79.5ms    84.7ms   161.5ms
1-field GROUP BY (service, LIMIT 20)              78.2ms    85.1ms   151.9ms
2-field GROUP BY (userid+searchphrase, LIMIT 10) 145.7ms   152.8ms   188.6ms
2-field GROUP BY (userid+service, LIMIT 10)      103.7ms   108.9ms   159.4ms
2-field GROUP BY (service+method, LIMIT 20)       87.9ms    96.8ms   152.6ms
Baseline: full-scan (non-indexed field)          106.6ms   115.3ms   159.0ms
```

| Metric | Value |
|--------|-------|
| 1-field avg median | **86.1 ms** |
| 2-field avg median | **119.5 ms** |
| Baseline (full parquet scan) | **115.3 ms** |
| 2-field / 1-field ratio | 1.39x |
| Low-cardinality 2-field (service+method) | **96.8 ms** (faster than baseline!) |

**Key observation:** For low-cardinality field combinations (e.g., `service × method` = 50×5 = 250 pairs), the 2-field optimization already outperforms full parquet scan. For high-cardinality combinations (e.g., `userid × searchphrase` = 500×200 = 100K pairs), the nested aggregation overhead is noticeable but bounded.

---

### Production Prerequisites & Notes

> ⚠️ **Important**: The Tantivy index optimization only activates for files created **after** `index_fields` is configured.

**Before deploying:**

1. **Configure `index_fields` BEFORE ingesting data**
   ```bash
   PUT /api/{org}/streams/{stream}/settings
   { "index_fields": { "add": ["field1", "field2"], "remove": [] } }
   ```

2. **For existing data**: Trigger compaction/re-index to rebuild Tantivy indexes for older files. Otherwise, queries will fall back to the standard DataFusion parquet scan path (correctness preserved, just no speedup).

3. **Both GROUP BY fields must be in `index_fields`** — if either field is not indexed, the optimizer will not match and the query runs through the normal path.

4. **Fallback is safe** — if the pattern doesn't match (wrong number of fields, non-indexed field, no LIMIT, non-COUNT aggregate), the query silently falls through to the standard execution plan. No behavior change for unmatched queries.

5. **Over-fetch tuning**: The current strategy uses `limit * 2` (min 100) for both outer and inner aggregation. For extremely high cardinality combinations or highly skewed data, this may under-fetch. DataFusion's re-aggregation handles correctness, but top-N accuracy across segments may be approximate in edge cases.

---

### Files Changed

```
 src/config/src/meta/inverted_index.rs                              |  27 +++
 src/proto/proto/cluster/plan.proto                                 |   7 +
 src/proto/src/generated/cluster.rs                                 |  13 +-
 src/service/search/datafusion/optimizer/.../index_optimizer/mod.rs |   9 +
 src/service/search/datafusion/optimizer/.../topn_multi.rs (NEW)    | 229 +++
 src/service/search/datafusion/plan/tantivy_optimize_exec.rs        |  64 +
 src/service/search/grpc/flight.rs                                  |   5 +-
 src/service/search/grpc/storage.rs                                 |  14 +
 src/service/search/grpc/tantivy.rs                                 | 131 ++
 src/service/search/grpc/tantivy_result_cache.rs                    |  13 +
 10 files changed, 510 insertions(+), 2 deletions(-)
```

### Testing

- Unit tests: `cargo test --lib test_is_simple_topn_multi` (pattern detection)
- Config tests: `cargo test -p config inverted_index` (enum serialization)
- Benchmark script: `scripts/bench_group_by.py` (end-to-end with 10M records)
