#!/usr/bin/env python3
"""
Benchmark script: Ingest ~10GB of log data into OpenObserve (COS backend).
Generates realistic log entries and sends them in batches via HTTP API.

Usage:
    python3 bench_cos_ingest.py --url http://<IP>:5080 --user admin@example.com --password YourPassword123!

Adjust TARGET_GB, BATCH_SIZE, CONCURRENCY as needed.
"""

import argparse
import json
import time
import random
import string
import threading
import requests
from datetime import datetime, timezone
from concurrent.futures import ThreadPoolExecutor, as_completed

# --- Configuration ---
TARGET_GB = 10
BATCH_SIZE = 5000          # records per HTTP request
CONCURRENCY = 8            # parallel upload threads
STREAM_NAME = "bench_cos"

# --- Log templates ---
LEVELS = ["info", "info", "info", "warning", "error", "debug", "info", "info"]
SERVICES = ["api-gateway", "auth-service", "payment-service", "order-service",
            "user-service", "notification-service", "search-service", "cache-service"]
METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH"]
PATHS = ["/api/v1/users", "/api/v1/orders", "/api/v1/products", "/api/v1/auth/login",
         "/api/v1/search", "/api/v1/payments", "/api/v1/notifications", "/healthz"]
STATUS_CODES = [200, 200, 200, 200, 201, 204, 301, 400, 401, 403, 404, 500, 502, 503]
MESSAGES = [
    "Request processed successfully",
    "User authenticated via OAuth2",
    "Database query completed in {ms}ms",
    "Cache hit ratio: {ratio}%",
    "Connection pool: {active}/{max} active",
    "Rate limit exceeded for client {ip}",
    "Timeout waiting for upstream response",
    "Failed to parse request body: invalid JSON",
    "SSL handshake completed with TLS 1.3",
    "Background job completed: {job}",
    "Memory usage: {mem}MB / {max_mem}MB",
    "Disk write latency spike detected: {latency}ms",
    "New deployment rolling out: version {version}",
    "Circuit breaker opened for {service}",
    "Retry attempt {n}/3 for downstream call",
]


def random_ip():
    return f"{random.randint(10,192)}.{random.randint(0,255)}.{random.randint(0,255)}.{random.randint(1,254)}"


def random_trace_id():
    return ''.join(random.choices(string.hexdigits[:16], k=32))


def generate_record(ts_base_us: int, offset: int) -> dict:
    level = random.choice(LEVELS)
    service = random.choice(SERVICES)
    msg_template = random.choice(MESSAGES)

    msg = msg_template.format(
        ms=random.randint(1, 500),
        ratio=random.randint(60, 99),
        active=random.randint(1, 50),
        max=100,
        ip=random_ip(),
        job=random.choice(["cleanup", "sync", "backup", "index_rebuild"]),
        mem=random.randint(200, 3800),
        max_mem=4096,
        latency=random.randint(50, 2000),
        version=f"v{random.randint(1,5)}.{random.randint(0,20)}.{random.randint(0,99)}",
        service=random.choice(SERVICES),
        n=random.randint(1, 3),
    )

    return {
        "_timestamp": ts_base_us + offset,
        "level": level,
        "service": service,
        "message": msg,
        "method": random.choice(METHODS),
        "path": random.choice(PATHS),
        "status_code": random.choice(STATUS_CODES),
        "duration_ms": random.randint(1, 5000),
        "client_ip": random_ip(),
        "trace_id": random_trace_id(),
        "user_id": f"user_{random.randint(1, 100000)}",
        "region": random.choice(["us-east-1", "eu-west-1", "ap-southeast-1", "ap-northeast-1"]),
        "host": f"{service}-{random.randint(1,10)}.prod.internal",
        "request_size": random.randint(100, 50000),
        "response_size": random.randint(50, 200000),
    }


def estimate_record_size() -> int:
    """Estimate average JSON record size in bytes."""
    sample = [generate_record(int(time.time() * 1e6), i) for i in range(100)]
    total = sum(len(json.dumps(r)) for r in sample)
    return total // 100


def send_batch(session: requests.Session, url: str, auth: tuple, records: list) -> tuple:
    """Send a batch of records. Returns (success_count, bytes_sent, duration_s)."""
    payload = json.dumps(records)
    bytes_sent = len(payload.encode())
    start = time.time()
    try:
        resp = session.post(
            f"{url}/api/default/{STREAM_NAME}/_json",
            data=payload,
            headers={"Content-Type": "application/json"},
            auth=auth,
            timeout=120,
        )
        duration = time.time() - start
        if resp.status_code == 200:
            result = resp.json()
            success = result.get("status", [{}])[0].get("successful", 0)
            return (success, bytes_sent, duration)
        else:
            print(f"  [ERROR] HTTP {resp.status_code}: {resp.text[:200]}")
            return (0, bytes_sent, duration)
    except Exception as e:
        duration = time.time() - start
        print(f"  [ERROR] {e}")
        return (0, bytes_sent, duration)


def main():
    parser = argparse.ArgumentParser(description="Ingest ~10GB into OpenObserve")
    parser.add_argument("--url", default="http://localhost:5080", help="OpenObserve URL")
    parser.add_argument("--user", default="admin@example.com", help="Username")
    parser.add_argument("--password", default="YourPassword123!", help="Password")
    parser.add_argument("--target-gb", type=float, default=TARGET_GB, help="Target data in GB")
    parser.add_argument("--batch-size", type=int, default=BATCH_SIZE, help="Records per batch")
    parser.add_argument("--concurrency", type=int, default=CONCURRENCY, help="Parallel threads")
    args = parser.parse_args()

    target_bytes = int(args.target_gb * 1024 * 1024 * 1024)
    auth = (args.user, args.password)

    # Estimate record size
    avg_size = estimate_record_size()
    total_records = target_bytes // avg_size
    total_batches = total_records // args.batch_size

    print(f"=== OpenObserve COS Ingestion Benchmark ===")
    print(f"Target:        {args.target_gb} GB (~{target_bytes / 1e9:.1f} GB)")
    print(f"Avg record:    ~{avg_size} bytes")
    print(f"Total records: ~{total_records:,}")
    print(f"Total batches: ~{total_batches:,}")
    print(f"Batch size:    {args.batch_size}")
    print(f"Concurrency:   {args.concurrency}")
    print(f"Stream:        {STREAM_NAME}")
    print(f"Endpoint:      {args.url}")
    print()

    # Verify connectivity
    try:
        resp = requests.get(f"{args.url}/healthz", timeout=5)
        print(f"Health check:  {resp.status_code} ✓")
    except Exception as e:
        print(f"Health check FAILED: {e}")
        return

    print()
    print("Starting ingestion...")
    print()

    total_sent_bytes = 0
    total_sent_records = 0
    total_failed = 0
    start_time = time.time()
    lock = threading.Lock()

    ts_base = int(time.time() * 1e6)  # microseconds

    def generate_and_send(batch_id: int):
        nonlocal total_sent_bytes, total_sent_records, total_failed, ts_base
        session = requests.Session()

        # Generate batch
        offset_base = batch_id * args.batch_size
        records = [generate_record(ts_base, offset_base + i) for i in range(args.batch_size)]

        # Send
        success, bytes_sent, duration = send_batch(session, args.url, auth, records)

        with lock:
            total_sent_bytes += bytes_sent
            total_sent_records += success
            if success == 0:
                total_failed += 1

        return (success, bytes_sent, duration)

    batch_id = 0
    with ThreadPoolExecutor(max_workers=args.concurrency) as executor:
        futures = []
        report_interval = max(1, total_batches // 20)  # report ~20 times

        while total_sent_bytes < target_bytes:
            # Submit a chunk of work
            chunk_size = min(args.concurrency * 2, total_batches - batch_id)
            if chunk_size <= 0:
                break

            for _ in range(chunk_size):
                futures.append(executor.submit(generate_and_send, batch_id))
                batch_id += 1

            # Collect completed futures
            done = []
            for f in as_completed(futures[:chunk_size]):
                done.append(f.result())
            futures = futures[chunk_size:]

            # Progress report
            if batch_id % report_interval == 0 or total_sent_bytes >= target_bytes:
                elapsed = time.time() - start_time
                gb_sent = total_sent_bytes / (1024**3)
                rate_mbps = (total_sent_bytes / 1e6) / elapsed if elapsed > 0 else 0
                rps = total_sent_records / elapsed if elapsed > 0 else 0
                pct = min(100, (total_sent_bytes / target_bytes) * 100)
                print(
                    f"  [{pct:5.1f}%] {gb_sent:.2f} GB sent | "
                    f"{total_sent_records:,} records | "
                    f"{rate_mbps:.1f} MB/s | "
                    f"{rps:.0f} records/s | "
                    f"{elapsed:.0f}s elapsed"
                )

        # Wait for remaining
        for f in as_completed(futures):
            f.result()

    elapsed = time.time() - start_time
    gb_sent = total_sent_bytes / (1024**3)
    rate_mbps = (total_sent_bytes / 1e6) / elapsed if elapsed > 0 else 0
    rps = total_sent_records / elapsed if elapsed > 0 else 0

    print()
    print(f"=== Results ===")
    print(f"Total sent:    {gb_sent:.2f} GB")
    print(f"Total records: {total_sent_records:,}")
    print(f"Failed batches:{total_failed}")
    print(f"Duration:      {elapsed:.1f}s")
    print(f"Throughput:    {rate_mbps:.1f} MB/s")
    print(f"Records/sec:   {rps:.0f}")
    print()
    print("Done! Check COS bucket for uploaded parquet files.")


if __name__ == "__main__":
    main()
