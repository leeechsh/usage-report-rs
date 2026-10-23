#!/usr/bin/env python3
"""Read-only benchmark of this project's running demonstration service."""
import json
import os
from pathlib import Path
import socket
import sqlite3
import statistics
import sys
import time

project = Path(__file__).resolve().parents[1]
database = project / "var/index.db"
endpoint = project / "var/report.sock"
durations = []
response = None
for _ in range(20):
    started = time.perf_counter()
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(15)
        client.connect(str(endpoint))
        client.sendall(json.dumps(dict(command="tree", uid=os.getuid(), generation=None,
                                      node=None, offset=0, limit=200)).encode() + b"\n")
        with client.makefile("rb") as stream:
            response = json.loads(stream.readline())
    durations.append((time.perf_counter() - started) * 1000)
    if response.get("error"):
        sys.exit(response["error"])
with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
    integrity = connection.execute("PRAGMA quick_check").fetchone()[0]
    row_counts = {table: connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
                  for table in ("nodes", "usage", "inodes")}
    query_plan = connection.execute(
        "EXPLAIN QUERY PLAN SELECT node FROM usage WHERE gen=? AND uid=? AND parent=? "
        "ORDER BY allocated DESC,node LIMIT 201",
        (response["payload"]["data"]["run"]["id"], os.getuid(), 1)).fetchall()
print(json.dumps(dict(
    scope="non-root home demonstration; not full root filesystem",
    iterations=20, median_query_ms=round(statistics.median(durations), 3),
    max_query_ms=round(max(durations), 3), database_bytes=database.stat().st_size,
    quick_check=integrity, row_counts=row_counts, query_plan=query_plan,
    indexed_allocated_bytes=response["payload"]["data"]["totals"]["allocated"],
    run=response["payload"]["data"]["run"],
), ensure_ascii=False, indent=2))
