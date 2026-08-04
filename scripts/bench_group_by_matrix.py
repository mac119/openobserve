#!/usr/bin/env python3
"""
Benchmark Matrix: 8 scenarios for two-field GROUP BY via Tantivy
================================================================
Compares tantivy-optimized path vs full parquet scan for each scenario.

Scenarios:
  1. 2 high cardinality, no filter, LIMIT 1000
  2. 2 high cardinality, no filter, LIMIT 10000
  3. 2 high cardinality, match_all, LIMIT 1000
  4. 2 high cardinality, match_all, LIMIT 10000
  5. 1 high + 1 low cardinality, no filter, LIMIT 1000
  6. 1 high + 1 low cardinality, no filter, LIMIT 10000
  7. 1 high + 1 low cardinality, match_all, LIMIT 1000
  8. 1 high + 1 low cardinality, match_all, LIMIT 10000

Usage:
    python3 scripts/bench_group_by_matrix.py [--rounds N]
"""

import argparse
import json
import time
import base64
from datetime import datetime, timedelta, timezone
from statistics import mean, median, stdev
from urllib.request import Request, urlopen
from urllib.error import HTTPError

# Config
BASE_URL = "http://localhost:5090"
USER = "admin@bench.com"
PASS = "Bench123!"
ORG = "default"
STREAM = "bench_group_by"

HEADERS = {
    "Authorization": "Basic " + base64.b64encode(f"{USER}:{PASS}".encode()).decode(),
    "Content-Type": "application/json",
}

# High cardinality fields: userid (500 values), searchphrase (200 values)
# Low cardinality field: method (5 values)
SCENARIOS = [
    {"id": 1, "label": "2 high cardinality, no filter, LIMIT 1000",
     "fields": ("userid", "searchphrase"), "filter": None, "limit": 1000},
    {"id": 2, "label": "2 high cardinality, no filter, LIMIT 10000",
     "fields": ("userid", "searchphrase"), "filter": None, "limit": 10000},
    {"id": 3, "label": "2 high cardinality, match_all, LIMIT 1000",
     "fields": ("userid", "searchphrase"), "filter": "match_all('user')", "limit": 1000},
    {"id": 4, "label": "2 high cardinality, match_all, LIMIT 10000",
     "fields": ("userid", "searchphrase"), "filter": "match_all('user')", "limit": 10000},
    {"id": 5, "label": "1 high + 1 low cardinality, no filter, LIMIT 1000",
     "fields": ("userid", "method"), "filter": None, "limit": 1000},
    {"id": 6, "label": "1 high + 1 low cardinality, no filter, LIMIT 10000",
     "fields": ("userid", "method"), "filter": None, "limit": 10000},
    {"id": 7, "label": "1 high + 1 low cardinality, match_all, LIMIT 1000",
     "fields": ("userid", "method"), "filter": "match_all('user')", "limit": 1000},
    {"id": 8, "label": "1 high + 1 low cardinality, match_all, LIMIT 10000",
     "fields": ("userid", "method"), "filter": "match_all('user')", "limit": 10000},
]


def api_request(path, method="GET", data=None):
    url = f"{BASE_URL}{path}"
    body = json.dumps(data).encode() if data else None
    req = Request(url, data=body, headers=HEADERS, method=method)
    try:
        with urlopen(req, timeout=120) as resp:
            return resp.status, json.loads(resp.read().decode())
    except HTTPError as e:
        body_text = e.read().decode()[:300] if hasattr(e, "read") else ""
        return e.code, {"error": str(e), "body": body_text}
    except Exception as e:
        return 0, {"error": str(e)}


def build_sql(fields, filter_clause, limit):
    f1, f2 = fields
    where = f" WHERE {filter_clause}" if filter_clause else ""
    return (
        f'SELECT {f1}, {f2}, COUNT(*) as cnt FROM "{STREAM}"'
        f"{where} GROUP BY {f1}, {f2} ORDER BY cnt DESC LIMIT {limit}"
    )


def run_query(sql, start_time, end_time):
    payload = {
        "query": {
            "sql": sql,
            "start_time": start_time,
            "end_time": end_time,
            "from": 0,
            "size": 100,
        }
    }
    t0 = time.time()
    status, resp = api_request(f"/api/{ORG}/_search?type=logs", method="POST", data=payload)
    elapsed_ms = (time.time() - t0) * 1000
    scan_size = resp.get("scan_size", 0) if status == 200 else -1
    hits = resp.get("total", 0) if status == 200 else 0
    return elapsed_ms, hits, scan_size


def set_index_fields(fields_list):
    """Set or clear index_fields on the stream."""
    if fields_list:
        settings = {"index_fields": {"add": fields_list, "remove": []}}
    else:
        # Remove all index fields
        current_fields = get_current_index_fields()
        if current_fields:
            settings = {"index_fields": {"add": [], "remove": current_fields}}
        else:
            return True
    status, resp = api_request(f"/api/{ORG}/streams/{STREAM}/settings", method="PUT", data=settings)
    return status == 200


def get_current_index_fields():
    status, resp = api_request(f"/api/{ORG}/streams/{STREAM}/schema")
    if status == 200:
        return resp.get("settings", {}).get("index_fields", [])
    return []


def run_benchmark(rounds, warmup=1):
    now = datetime.now(timezone.utc)
    start_us = int((now - timedelta(hours=48)).timestamp() * 1_000_000)
    end_us = int(now.timestamp() * 1_000_000)

    results = []

    # First: run ALL scenarios with tantivy (index_fields stays set)
    print("\n" + "="*80)
    print("  PHASE 1: Running all scenarios WITH tantivy optimization")
    print("="*80)
    set_index_fields(["userid", "searchphrase", "service", "method", "status"])
    time.sleep(2)

    tantivy_results = {}
    for scenario in SCENARIOS:
        sql = build_sql(scenario["fields"], scenario["filter"], scenario["limit"])
        print(f"\n  Scenario {scenario['id']}: {scenario['label']}")

        for _ in range(warmup):
            run_query(sql, start_us, end_us)

        latencies = []
        for r in range(rounds):
            ms, hits, scan_size = run_query(sql, start_us, end_us)
            latencies.append(ms)
            print(f"    round {r+1}: {ms:>8.1f} ms (hits={hits}, scan={scan_size}MB)")
        tantivy_results[scenario['id']] = latencies

    # Second: remove index_fields ONCE, wait, then run ALL scenarios without tantivy
    print("\n" + "="*80)
    print("  PHASE 2: Running all scenarios WITHOUT tantivy (parquet only)")
    print("  Removing index_fields...")
    print("="*80)
    set_index_fields([])
    time.sleep(3)

    parquet_results = {}
    for scenario in SCENARIOS:
        sql = build_sql(scenario["fields"], scenario["filter"], scenario["limit"])
        print(f"\n  Scenario {scenario['id']}: {scenario['label']}")

        for _ in range(warmup):
            run_query(sql, start_us, end_us)

        latencies = []
        for r in range(rounds):
            ms, hits, scan_size = run_query(sql, start_us, end_us)
            latencies.append(ms)
            print(f"    round {r+1}: {ms:>8.1f} ms (hits={hits}, scan={scan_size}MB)")
        parquet_results[scenario['id']] = latencies

    # Restore index_fields
    print("\n\nRestoring index_fields...")
    set_index_fields(["userid", "searchphrase", "service", "method", "status"])

    # Build results
    for scenario in SCENARIOS:
        t_lats = tantivy_results[scenario['id']]
        p_lats = parquet_results[scenario['id']]
        t_median = median(t_lats)
        p_median = median(p_lats)
        speedup = p_median / t_median if t_median > 0 else 0

        results.append({
            "id": scenario["id"],
            "label": scenario["label"],
            "fields": scenario["fields"],
            "filter": scenario["filter"],
            "limit": scenario["limit"],
            "tantivy": {
                "latencies": t_lats,
                "min": min(t_lats),
                "median": t_median,
                "mean": mean(t_lats),
                "max": max(t_lats),
            },
            "parquet": {
                "latencies": p_lats,
                "min": min(p_lats),
                "median": p_median,
                "mean": mean(p_lats),
                "max": max(p_lats),
            },
            "speedup": speedup,
        })

        print(f"\n  → tantivy median: {t_median:.1f} ms | parquet median: {p_median:.1f} ms | speedup: {speedup:.2f}x")

    return results


def print_report(results):
    print("\n\n")
    print("=" * 100)
    print("  BENCHMARK MATRIX REPORT — Tantivy vs Full Parquet Scan")
    print("=" * 100)
    print(f"{'#':<3} {'Scenario':<50} {'Tantivy':>10} {'Parquet':>10} {'Speedup':>10}")
    print("-" * 100)

    for r in results:
        print(
            f"{r['id']:<3} {r['label']:<50} "
            f"{r['tantivy']['median']:>8.1f}ms "
            f"{r['parquet']['median']:>8.1f}ms "
            f"{r['speedup']:>9.2f}x"
        )

    print("-" * 100)
    avg_speedup = mean([r["speedup"] for r in results])
    print(f"{'':3} {'Average':50} {'':>10} {'':>10} {avg_speedup:>9.2f}x")
    print("=" * 100)


def main():
    parser = argparse.ArgumentParser(description="Benchmark matrix: tantivy vs parquet")
    parser.add_argument("--rounds", type=int, default=5, help="Rounds per scenario per mode")
    parser.add_argument("--warmup", type=int, default=1, help="Warmup rounds (not counted)")
    args = parser.parse_args()

    # Health check
    status, _ = api_request("/healthz")
    if status != 200:
        print("ERROR: OpenObserve not reachable")
        return

    # Get stream info
    status, resp = api_request(f"/api/{ORG}/streams/{STREAM}/schema")
    if status == 200:
        s = resp["stats"]
        print(f"Stream: {STREAM}")
        print(f"  Records:         {s['doc_num']:,}")
        print(f"  Original size:   {s['storage_size']:.2f} MB")
        print(f"  Compressed size: {s['compressed_size']:.2f} MB")
        print(f"  Index size:      {s['index_size']:.2f} MB")
        print(f"  Files:           {s['file_num']}")
    else:
        print(f"ERROR: stream {STREAM} not found")
        return

    results = run_benchmark(args.rounds, args.warmup)
    print_report(results)

    # Save results
    out_file = "scripts/bench_group_by_matrix_results.json"
    with open(out_file, "w") as f:
        json.dump({
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "rounds": args.rounds,
            "results": results,
        }, f, indent=2)
    print(f"\nRaw results saved to {out_file}")


if __name__ == "__main__":
    main()
