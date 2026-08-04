# Implementation Plan: Live Tailing for Logs (#3148)

## Problem Statement

Currently, OpenObserve's "live mode" is a `setInterval` polling mechanism (min 5s) that **fully replaces** the result set on each refresh. This causes:
1. Loss of scroll position
2. Loss of currently-viewed log entries (only top N shown)
3. Jarring UX (entire table flashes on each refresh)

**Goal**: Implement true live tailing — new log entries **append** to the existing list in real-time, similar to `tail -f`.

---

## Current Architecture (What We Have)

| Layer | Mechanism | Status |
|-------|-----------|--------|
| **Auto-refresh** | `setInterval` + full `getQueryData()` | ✅ Active (5s-1day intervals) |
| **HTTP/2 Streaming** | `/_search_stream` SSE endpoint + Web Worker | ✅ Active (for one-shot search) |
| **WebSocket** | `useWebSocket.ts` / `useSearchWebSocket.ts` | ⚠️ Code exists but **disabled** (`isWebSocketEnabled = false`) |
| **Virtual scroll** | `@tanstack/vue-virtual` in TenstackTable | ✅ Active |
| **Backend streaming** | `tokio::mpsc` channel → SSE response | ✅ Active (bounded query) |

### Key Insight

The existing `_search_stream` endpoint is **query-bounded** — it searches a fixed time range and closes when results are complete. What we need is a **subscription-style** endpoint that stays open and pushes new log entries as they arrive.

---

## Proposed Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Frontend                                   │
│                                                                   │
│  ┌─────────────┐    ┌──────────────┐    ┌──────────────────┐    │
│  │ SearchBar   │    │ useLiveTail  │    │ TenstackTable    │    │
│  │ [▶ Tail]    │───▶│ composable   │───▶│ + auto-scroll    │    │
│  └─────────────┘    └──────┬───────┘    └──────────────────┘    │
│                            │                                      │
│                    ┌───────▼────────┐                            │
│                    │  Web Worker    │  (reuse streamWorker.js)   │
│                    │  SSE parsing   │                            │
│                    └───────┬────────┘                            │
└────────────────────────────┼────────────────────────────────────┘
                             │ HTTP/2 SSE (long-lived connection)
┌────────────────────────────▼────────────────────────────────────┐
│                        Backend                                    │
│                                                                   │
│  ┌──────────────────┐    ┌────────────────────┐                 │
│  │  /api/{org}/      │    │  TailSubscriber    │                 │
│  │  {stream}/_tail   │───▶│  (watch WAL/ingestion channel)      │
│  │  (SSE endpoint)   │    │  → filter by query │                 │
│  └──────────────────┘    │  → push matching    │                 │
│                           │    entries via mpsc  │                 │
│                           └────────────────────┘                 │
│                                    ↑                              │
│                           ┌────────┴────────┐                    │
│                           │ Ingestion Event  │                    │
│                           │ Broadcast Channel│                    │
│                           └─────────────────┘                    │
└──────────────────────────────────────────────────────────────────┘
```

---

## Implementation Plan

### Phase 1: Backend — Tail Subscription Endpoint

**New file**: `src/handler/http/request/search/tail.rs`  
**New file**: `src/service/search/tail.rs`

#### 1.1 Ingestion Broadcast Channel

In the ingestion path (`src/ingester/`), after writing to MemTable, broadcast the new records to a `tokio::sync::broadcast` channel:

```rust
// In src/ingester/src/writer.rs or src/ingester/src/lib.rs
use tokio::sync::broadcast;

lazy_static! {
    // Channel for live tail subscribers; capacity = recent N batches
    pub static ref TAIL_BROADCAST: broadcast::Sender<TailEvent> = {
        let (tx, _) = broadcast::channel(1024);
        tx
    };
}

pub struct TailEvent {
    pub org_id: String,
    pub stream_name: String,
    pub stream_type: StreamType,
    pub records: Vec<json::Value>,  // or Arc<RecordBatch>
    pub timestamp: i64,
}
```

After successful MemTable write, emit:
```rust
let _ = TAIL_BROADCAST.send(TailEvent { org_id, stream_name, records, .. });
```

#### 1.2 Tail SSE Endpoint

**Route**: `POST /api/{org_id}/{stream_name}/_tail`

**Request body**:
```json
{
  "sql": "SELECT * FROM stream WHERE level = 'error'",  // optional filter
  "max_records_per_batch": 50,   // max records per SSE event
  "flush_interval_ms": 1000      // min interval between pushes
}
```

**Handler logic**:
1. Subscribe to `TAIL_BROADCAST`
2. Filter incoming `TailEvent` by `org_id` + `stream_name`
3. If SQL filter provided, evaluate WHERE clause against each record (lightweight in-memory filter using DataFusion's `RowFilter` or simple predicate pushdown)
4. Batch matching records and flush every `flush_interval_ms`
5. Send as SSE: `event: tail_records\ndata: {json_array}\n\n`
6. Keep connection alive with periodic heartbeat: `event: heartbeat\ndata: {}\n\n`
7. Close on client disconnect (AbortController) or server shutdown

**Connection lifecycle**:
```
Client connects → subscribe to broadcast → filter loop → SSE push
                                                      ↓
Client disconnects (AbortSignal) → unsubscribe → cleanup
```

#### 1.3 Cluster Mode

In HA cluster mode:
- **Router** node receives the `_tail` request
- Router subscribes to NATS JetStream subject (e.g., `o2_tail.{org_id}.{stream_name}`)
- Each **Ingester** publishes `TailEvent` to NATS after MemTable write
- Router receives from NATS → filters → pushes to client SSE

This ensures tailing works regardless of which Ingester receives the data.

---

### Phase 2: Frontend — useLiveTail Composable

**New file**: `web/src/composables/useLogs/useLiveTail.ts`

```typescript
interface TailOptions {
  orgId: string;
  streamName: string;
  sql?: string;
  maxBuffer: number;        // max entries to keep in memory (e.g., 5000)
  flushIntervalMs: number;  // backend flush interval
}

export function useLiveTail(options: TailOptions) {
  const entries = ref<LogEntry[]>([]);
  const isActive = ref(false);
  const isPaused = ref(false);  // user scrolled up → pause auto-scroll
  const connectionStatus = ref<'connecting' | 'connected' | 'disconnected'>('disconnected');
  
  let abortController: AbortController | null = null;
  let worker: Worker | null = null;

  async function start() {
    isActive.value = true;
    connectionStatus.value = 'connecting';
    abortController = new AbortController();
    
    // Reuse existing streamWorker.js pattern
    const url = `${store.state.API_ENDPOINT}/api/${options.orgId}/${options.streamName}/_tail`;
    
    const response = await fetch(url, {
      method: 'POST',
      headers: { Authorization: ... },
      body: JSON.stringify({ sql: options.sql, flush_interval_ms: options.flushIntervalMs }),
      signal: abortController.signal,
    });
    
    connectionStatus.value = 'connected';
    const reader = response.body!.getReader();
    
    // Parse SSE stream (same pattern as streamWorker.js)
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      
      const events = parseSSE(decode(value));
      for (const event of events) {
        if (event.type === 'tail_records') {
          appendRecords(JSON.parse(event.data));
        }
      }
    }
  }

  function appendRecords(newRecords: LogEntry[]) {
    // Append to the END of the list (newest at bottom for tail mode)
    entries.value.push(...newRecords);
    
    // Trim buffer if exceeds maxBuffer (remove oldest)
    if (entries.value.length > options.maxBuffer) {
      entries.value.splice(0, entries.value.length - options.maxBuffer);
    }
  }

  function stop() {
    abortController?.abort();
    isActive.value = false;
    connectionStatus.value = 'disconnected';
  }

  function clear() {
    entries.value = [];
  }

  return { entries, isActive, isPaused, connectionStatus, start, stop, clear };
}
```

---

### Phase 3: Frontend — UI Integration

#### 3.1 Tail Toggle Button

In `SearchBar.vue`, add a "Live Tail" button next to the existing auto-refresh selector:

```vue
<OButton
  v-if="!liveTail.isActive"
  @click="startTail"
  variant="ghost"
  size="sm"
>
  <OIcon name="play" /> Live Tail
</OButton>

<OButton
  v-else
  @click="stopTail"
  variant="danger-ghost"
  size="sm"
>
  <OIcon name="stop" /> Stop ({{ liveTail.entries.length }} entries)
</OButton>
```

#### 3.2 Tail Mode in SearchResult.vue

When live tail is active:
- Replace the table data source: `rows = liveTail.entries` (instead of `searchObj.data.queryResults.hits`)
- **Sort order**: newest at bottom (reverse of default search)
- Show a "tail mode" indicator bar with stats (entries/sec, connection status)
- Disable pagination controls
- Disable time range picker (tailing is always "now")

#### 3.3 Auto-Scroll in TenstackTable.vue

Add auto-scroll behavior when in tail mode:

```typescript
// New prop
props: {
  autoScrollToBottom: { type: Boolean, default: false },
}

// Watch for new data + auto-scroll
watch(() => props.rows.length, () => {
  if (props.autoScrollToBottom && !isPausedByUser.value) {
    nextTick(() => {
      rowVirtualizer.value.scrollToIndex(formattedRows.value.length - 1, {
        align: 'end',
        behavior: 'smooth',
      });
    });
  }
});

// Pause auto-scroll when user scrolls up
function handleScroll(event: Event) {
  const el = event.target as HTMLElement;
  const isAtBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
  isPausedByUser.value = !isAtBottom;
}

// "Jump to latest" button when paused
// <OButton v-if="isPausedByUser" @click="scrollToBottom">↓ Jump to latest</OButton>
```

#### 3.4 UX Details

| Aspect | Behavior |
|--------|----------|
| **Entry order** | Newest at bottom (tail mode), newest at top (search mode) |
| **Buffer limit** | Keep last 5000 entries in memory; older entries discarded |
| **Scroll pause** | User scrolls up → pause auto-scroll; show "↓ Jump to latest" button |
| **Filter** | Tail with current SQL WHERE clause; changes require restart |
| **Connection lost** | Show reconnecting indicator; auto-retry with exponential backoff |
| **Stop** | Entries remain visible; user can scroll through history |
| **Clear** | Button to wipe buffer and start fresh |

---

### Phase 4: Graceful Fallback

For environments where the `_tail` endpoint is not available (older backend versions), fall back to the current polling mechanism but with **incremental append** behavior:

```typescript
// Fallback: polling-based pseudo-tail
async function pollTail() {
  const lastTimestamp = entries.value.at(-1)?._timestamp ?? Date.now() * 1000;
  
  // Query only records AFTER the last known timestamp
  const result = await search({
    sql: `SELECT * FROM ${stream} WHERE _timestamp > ${lastTimestamp} ORDER BY _timestamp ASC LIMIT 100`,
  });
  
  if (result.hits.length > 0) {
    appendRecords(result.hits);  // Append, don't replace
  }
  
  setTimeout(pollTail, 2000);  // Poll every 2s
}
```

This gives the "append" UX even without the streaming backend, just with higher latency.

---

## Comparison with Existing Solutions

| System | Mechanism | Latency |
|--------|-----------|---------|
| **Grafana Loki** | WebSocket `/loki/api/v1/tail` | ~1s |
| **Datadog Live Tail** | WebSocket subscription | ~2s |
| **Kibana Discover** | Polling-based "auto-refresh" | 5s+ |
| **CloudWatch Live Tail** | HTTP/2 streaming | ~1s |
| **This proposal** | HTTP/2 SSE `/_tail` | ~1s (configurable flush interval) |

We chose **HTTP/2 SSE over WebSocket** because:
1. OpenObserve already has mature SSE infrastructure (`_search_stream`)
2. SSE works through all proxies/load balancers without special config
3. Unidirectional (server→client) is all we need for tailing
4. Existing Web Worker can be reused with minimal changes

---

## Open Questions

1. **Buffer management**: Should the backend limit how far back a tail subscriber can go? (Proposal: tail only shows data arriving AFTER subscription starts — no historical backfill)
2. **Multi-stream tailing**: Should we support tailing multiple streams simultaneously? (Proposal: one connection per stream initially; multi-stream in v2)
3. **Rate limiting**: High-throughput streams could overwhelm the browser. Should we enforce server-side sampling for streams exceeding X events/sec? (Proposal: yes, configurable `max_records_per_second` with a "sampled" indicator in UI)
4. **Cluster mode priority**: Is NATS pub/sub for cross-ingester tailing a P0 requirement, or can we start with single-node only?

---

## File Inventory (Estimated)

| File | Type | Description |
|------|------|-------------|
| `src/handler/http/request/search/tail.rs` | New | HTTP handler for `/_tail` endpoint |
| `src/service/search/tail.rs` | New | Tail subscription service logic |
| `src/ingester/src/broadcast.rs` | New | `TAIL_BROADCAST` channel + `TailEvent` |
| `src/handler/http/router/mod.rs` | Modify | Register `/_tail` route |
| `src/ingester/src/writer.rs` | Modify | Emit to broadcast after MemTable write |
| `web/src/composables/useLogs/useLiveTail.ts` | New | Core tail composable |
| `web/src/plugins/logs/SearchBar.vue` | Modify | Add tail toggle button |
| `web/src/plugins/logs/SearchResult.vue` | Modify | Tail mode data binding |
| `web/src/plugins/logs/TenstackTable.vue` | Modify | Auto-scroll + scroll-pause |
| `web/src/services/streaming_search.ts` | Modify | Add `tail()` API method |
