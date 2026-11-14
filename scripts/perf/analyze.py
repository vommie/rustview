#!/usr/bin/env python3
"""Reads logs of RustView's performance probe (RUSTVIEW_PERF_PLAN, see docs/perf/README.md) and prints, per scenario,
frame pacing, the cost of the UI thread, the input latency and how many visible tiles had no thumbnail.

usage: analyze.py LOG [LOG ...]             one table per log
       analyze.py --median LOG [LOG ...]    one table: the median of every value over the logs (runs of one variant)
       analyze.py --json LOG [LOG ...]      the numbers as JSON
       analyze.py --compare A.json B.json   medians of variant A against variant B (made with --median --json)

Times are milliseconds. Columns:
  frames, fps      frames drawn in the scenario and frames per second
  gap p50/p95/p99/max   time from one frame to the next (16.7 ms = 60 fps)
  late             frames that came later than 1.5 display intervals (dropped frames)
  ui p50/p95/max   CPU time of the UI thread per frame (events, callbacks, layout, drawing calls): the app's own cost
  draw p50         part of it spent inside Slint's drawing
  lat p50/p95/max  from a pointer event to the end of the frame that shows it
  blank%           share of visible tile-frames without a thumbnail; blankmax = most blank tiles in one frame
  cpu%             CPU of the whole process (all threads) relative to one core; rss = memory at the end, MB
"""
import json
import statistics
import sys

REFRESH_MS = 1000.0 / 60.0


def pct(values, p):
    if not values:
        return 0.0
    ordered = sorted(values)
    k = (len(ordered) - 1) * p / 100.0
    lo = int(k)
    hi = min(lo + 1, len(ordered) - 1)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (k - lo)


def parse(path):
    frames, marks, events, info = [], [], [], {}
    with open(path, encoding="utf-8", errors="replace") as log:
        for line in log:
            part = line.split()
            if not part:
                continue
            try:
                if part[0] == "F" and len(part) >= 8:
                    frames.append(tuple(int(x) for x in part[1:8]))
                elif part[0] == "S" and len(part) >= 6:
                    marks.append((int(part[1]), part[2], part[3], int(part[4]), int(part[5])))
                elif part[0] == "E" and len(part) >= 4:
                    events.append((int(part[1]), part[2], int(part[3])))
                elif part[0] == "I" and len(part) >= 3:
                    info[part[1]] = " ".join(part[2:])
            except ValueError:
                continue  # a line cut off by a kill
    return frames, marks, events, info


def scenario_stats(frames, begin, end):
    t0, _, _, rss0, cpu0 = begin
    t1, _, _, rss1, cpu1 = end
    inside = [f for f in frames if t0 <= f[0] <= t1]
    seconds = max((t1 - t0) / 1e6, 1e-9)
    gaps = [(b[0] - a[0]) / 1000.0 for a, b in zip(inside, inside[1:])]
    ui = [f[3] / 1000.0 for f in inside[1:]]
    draw = [f[2] / 1000.0 for f in inside]
    lat = [f[4] / 1000.0 for f in inside if f[4] >= 0]
    tiles = [(f[5], f[6]) for f in inside if f[6] > 0]
    shown = sum(s for _, s in tiles)
    return {
        "seconds": round(seconds, 3),
        "frames": len(inside),
        "fps": round(len(inside) / seconds, 1),
        "gap_p50": round(pct(gaps, 50), 2),
        "gap_p95": round(pct(gaps, 95), 2),
        "gap_p99": round(pct(gaps, 99), 2),
        "gap_max": round(max(gaps, default=0.0), 2),
        "late": sum(1 for g in gaps if g > 1.5 * REFRESH_MS),
        "ui_p50": round(pct(ui, 50), 2),
        "ui_p95": round(pct(ui, 95), 2),
        "ui_max": round(max(ui, default=0.0), 2),
        "ui_total_ms": round(sum(ui), 1),
        "draw_p50": round(pct(draw, 50), 2),
        "lat_p50": round(pct(lat, 50), 2),
        "lat_p95": round(pct(lat, 95), 2),
        "lat_max": round(max(lat, default=0.0), 2),
        "blank_pct": round(100.0 * sum(b for b, _ in tiles) / shown, 2) if shown else 0.0,
        "blank_max": max((b for b, _ in tiles), default=0),
        "blank_frames_pct": round(100.0 * sum(1 for b, _ in tiles if b > 0) / len(tiles), 1) if tiles else 0.0,
        "cpu_pct": round(100.0 * (cpu1 - cpu0) / 1e6 / seconds, 1),
        "rss_mb": round(rss1 / 1024.0, 1),
        "rss_delta_mb": round((rss1 - rss0) / 1024.0, 1),
    }


def load_stats(frames, events):
    """Per list load (a folder switch, a sort, a jump): the times until the list, the first and all thumbnails are shown."""
    loads = []
    starts = [e for e in events if e[1] in ("folder_select", "sort_click", "jump_to")]
    for index, (t0, kind, value) in enumerate(starts):
        t_next = starts[index + 1][0] if index + 1 < len(starts) else float("inf")
        commit = next((e for e in events if e[1] == "list_committed" and t0 <= e[0] < t_next), None)
        base = commit[0] if commit else t0
        after = [f for f in frames if base <= f[0] < t_next]
        first_frame = after[0] if after else None
        first_thumb = next((f for f in after if f[6] > 0 and f[5] < f[6]), None)
        full = next((f for f in after if f[6] > 0 and f[5] == 0), None)

        def ms(frame):
            return round((frame[0] + frame[1] - t0) / 1000.0, 1) if frame else None

        loads.append({
            "kind": kind,
            "value": value,
            "rows": commit[2] if commit else None,
            "list_ms": round((commit[0] - t0) / 1000.0, 1) if commit else None,
            "frame_ms": ms(first_frame),
            "first_thumb_ms": ms(first_thumb),
            "all_thumbs_ms": ms(full),
        })
    return loads


def analyze(path):
    frames, marks, events, info = parse(path)
    scenarios, open_marks = {}, {}
    for mark in marks:
        if mark[1] == "begin":
            open_marks[mark[2]] = mark
        elif mark[2] in open_marks:
            scenarios[mark[2]] = scenario_stats(frames, open_marks.pop(mark[2]), mark)
    other = {}
    for _, name, value in events:
        if name not in ("list_committed", "folder_select", "sort_click", "jump_to"):
            other.setdefault(name, []).append(value)
    return {"log": path, "info": info, "scenarios": scenarios, "loads": load_stats(frames, events), "events": other}


def median_of(results):
    sizes = {r["info"].get("window") for r in results}
    if len(sizes) > 1:
        print(f"WARNING: the runs have different window sizes {sorted(map(str, sizes))}: their numbers do not compare",
              file=sys.stderr)
    merged = {"log": f"median of {len(results)} runs", "info": results[0]["info"], "scenarios": {}, "loads": [], "events": {}}
    for name in results[0]["scenarios"]:
        rows = [r["scenarios"][name] for r in results if name in r["scenarios"]]
        merged["scenarios"][name] = {key: round(statistics.median(row[key] for row in rows), 2) for key in rows[0]}
    for index, load in enumerate(results[0]["loads"]):
        row = dict(load)
        for key in ("list_ms", "frame_ms", "first_thumb_ms", "all_thumbs_ms"):
            values = [r["loads"][index][key] for r in results if index < len(r["loads"]) and r["loads"][index][key] is not None]
            row[key] = round(statistics.median(values), 1) if values else None
        merged["loads"].append(row)
    for name in results[0]["events"]:
        values = [v for r in results for v in r["events"].get(name, [])]
        merged["events"][name] = [round(statistics.median(values))] if values else []
    return merged


COLUMNS = [
    ("frames", "frames"), ("fps", "fps"), ("gap_p50", "gap50"), ("gap_p95", "gap95"), ("gap_p99", "gap99"),
    ("gap_max", "gapmax"), ("late", "late"), ("ui_p50", "ui50"), ("ui_p95", "ui95"), ("ui_max", "uimax"),
    ("draw_p50", "draw50"), ("lat_p50", "lat50"), ("lat_p95", "lat95"), ("lat_max", "latmax"), ("blank_pct", "blank%"),
    ("blank_max", "blankmax"), ("cpu_pct", "cpu%"), ("rss_mb", "rss"),
]


def show(result):
    print(f"== {result['log']}  {result['info']}")
    width = max([len(n) for n in result["scenarios"]] + [8])
    print(" " * width + "".join(f"{title:>9}" for _, title in COLUMNS))
    for name, row in result["scenarios"].items():
        print(f"{name:<{width}}" + "".join(f"{row[key]:>9}" for key, _ in COLUMNS))
    if result["loads"]:
        print("loads (ms from the request): list = rows ready, frame = drawn, first/all = thumbnails shown")
        for load in result["loads"]:
            print(f"  {load['kind']:<14}{load['value']:>10}  rows {load['rows']}  list {load['list_ms']}  frame {load['frame_ms']}"
                  f"  first {load['first_thumb_ms']}  all {load['all_thumbs_ms']}")
    for name, values in result["events"].items():
        print(f"  event {name}: {values}")


def compare(path_a, path_b):
    with open(path_a, encoding="utf-8") as a, open(path_b, encoding="utf-8") as b:
        left, right = json.load(a), json.load(b)
    if left["info"].get("window") != right["info"].get("window"):
        print(f"WARNING: different window sizes ({left['info'].get('window')} and {right['info'].get('window')}):"
              " the numbers do not compare", file=sys.stderr)
    keys = ["fps", "gap_p95", "gap_max", "late", "ui_p50", "ui_p95", "ui_max", "lat_p95", "blank_pct", "cpu_pct", "rss_mb"]
    names = [n for n in left["scenarios"] if n in right["scenarios"]]
    width = max([len(n) for n in names] + [8])
    print(" " * width + "".join(f"{key:>20}" for key in keys))
    for name in names:
        cells = "".join(f"{left['scenarios'][name][key]:>9} >{right['scenarios'][name][key]:>9}" for key in keys)
        print(f"{name:<{width}}{cells}")
    for index, load in enumerate(left["loads"]):
        if index < len(right["loads"]):
            other = right["loads"][index]
            print(f"  {load['kind']:<12}{load['value']:>8}  " + "  ".join(
                f"{key} {load[key]} > {other[key]}" for key in ("list_ms", "frame_ms", "first_thumb_ms", "all_thumbs_ms")))


def main():
    args = sys.argv[1:]
    if not args or args[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    if args[0] == "--compare":
        compare(args[1], args[2])
        return 0
    as_json = "--json" in args
    median = "--median" in args
    results = [analyze(path) for path in args if not path.startswith("--")]
    if median:
        results = [median_of(results)]
    if as_json:
        json.dump(results[0] if len(results) == 1 else results, sys.stdout, indent=1)
        print()
    else:
        for result in results:
            show(result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
