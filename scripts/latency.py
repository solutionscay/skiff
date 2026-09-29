#!/usr/bin/env python3
"""Summarizes a SKIFF_TRACE keystroke latency file.

    python3 scripts/latency.py /tmp/skiff-trace.jsonl [--since-min N]

Each stage gets count, p50, p90, p99 and max in ms, split into keys typed
into a quiet session and keys typed while the session was printing.
"""
import json
import sys
import time

STAGES = ["queue", "ipc", "echo", "parse", "render", "total"]


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(p / 100 * len(xs)))]


def table(rows, title):
    print(f"\n{title}: {len(rows)} keys")
    print(f"  {'stage':<7} {'n':>5} {'p50':>7} {'p90':>7} {'p99':>7} {'max':>7}")
    for s in STAGES:
        xs = [r[s] for r in rows if r.get(s) is not None]
        if not xs:
            print(f"  {s:<7} {0:>5}")
            continue
        print(f"  {s:<7} {len(xs):>5} {pct(xs, 50):>7.2f} {pct(xs, 90):>7.2f} {pct(xs, 99):>7.2f} {max(xs):>7.2f}")
    missing = sum(1 for r in rows if r.get("echo") is None)
    if missing:
        print(f"  no echo within 1 s: {missing}")


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    since = None
    if "--since-min" in args:
        i = args.index("--since-min")
        since = (time.time() - float(args[i + 1]) * 60) * 1000
        del args[i : i + 2]
    rows = [json.loads(l) for l in open(args[0]) if l.strip()]
    if since:
        rows = [r for r in rows if r["at"] >= since]
    table([r for r in rows if not r["busy"]], "Quiet session")
    table([r for r in rows if r["busy"]], "Session printing")


if __name__ == "__main__":
    main()
