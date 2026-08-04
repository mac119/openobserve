#!/usr/bin/env python3
"""
Periodic data ingestion for bench_group_by stream.
Designed to run via crontab to simulate production data flow.

Usage:
    python3 scripts/ingest_bench_data.py [--records N] [--target ha|local]

Default: ingest 500,000 records to HA cluster (port 5090)
"""

import argparse
import json
import random
import time
import base64
from datetime import datetime, timezone
from urllib.request import Request, urlopen
from urllib.error import HTTPError

TARGETS = {
    "ha": {"url": "http://localhost:5090", "user": "admin@bench.com", "pass": "Bench123!"},
    "local": {"url": "http://localhost:5080", "user": "admin@bench.com", "pass": "Bench123!"},
}

STREAM = "bench_group_by"
ORG = "default"

USERIDS = [f"user_{i:04d}" for i in range(500)]
SEARCH_PHRASES = [f"phrase_{i:03d}" for i in range(200)]
STATUSES = ["200", "301", "400", "403", "404", "500"]
METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH"]
SERVICES = [f"svc_{i:02d}" for i in range(50)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--records", type=int, default=500000)
    parser.add_argument("--target", choices=["ha", "local"], default="ha")
    parser.add_argument("--batch-size", type=int, default=5000)
    args = parser.parse_args()

    target = TARGETS[args.target]
    base_url = target["url"]
    headers = {
        "Authorization": "Basic " + base64.b64encode(f"{target['user']}:{target['pass']}".encode()).decode(),
        "Content-Type": "application/json",
    }

    # Health check
    try:
        req = Request(f"{base_url}/healthz", headers=headers)
        with urlopen(req, timeout=10) as resp:
            if resp.status != 200:
                print(f"[{datetime.now()}] ERROR: {args.target} not healthy")
                return
    except Exception as e:
        print(f"[{datetime.now()}] ERROR: cannot reach {args.target}: {e}")
        return

    # Ingest
    total = args.records
    batch_size = args.batch_size
    ingested = 0
    start_t = time.time()

    print(f"[{datetime.now()}] Ingesting {total:,} records to {args.target} ({base_url})")

    while ingested < total:
        this_batch = min(batch_size, total - ingested)
        now_us = int(datetime.now(timezone.utc).timestamp() * 1_000_000)
        records = [{
            "_timestamp": now_us,
            "userid": random.choice(USERIDS),
            "searchphrase": random.choice(SEARCH_PHRASES),
            "status": random.choice(STATUSES),
            "method": random.choice(METHODS),
            "service": random.choice(SERVICES),
            "response_time": random.randint(1, 5000),
            "message": f"req {random.choice(USERIDS)} {random.choice(SEARCH_PHRASES)}",
        } for _ in range(this_batch)]

        try:
            req = Request(
                f"{base_url}/api/{ORG}/{STREAM}/_json",
                data=json.dumps(records).encode(),
                headers=headers,
                method="POST",
            )
            with urlopen(req, timeout=60) as resp:
                pass
        except HTTPError as e:
            print(f"  Error at {ingested}: {e.code}")
            time.sleep(2)
            continue

        ingested += this_batch

    elapsed = time.time() - start_t
    print(f"[{datetime.now()}] Done: {ingested:,} records in {elapsed:.1f}s ({ingested/elapsed:,.0f} rec/s)")


if __name__ == "__main__":
    main()
