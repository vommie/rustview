#!/usr/bin/env python3
"""Reads a CPU profile recorded with samply and prints where a thread spent its time.

record:  samply record --save-only --unstable-presymbolicate -o run.json.gz -- target/debug/rustview
         (with RUSTVIEW_PERF_PLAN set, see docs/perf/README.md; needs kernel.perf_event_paranoid <= 1)
read:    profile.py run.json.gz [--thread NAME] [--top N] [--under TEXT] [--callers TEXT] [--depth N]

  --thread NAME   the thread to read (default: the main thread)
  --under TEXT    count only samples whose stack contains a function with TEXT in its name
  --callers TEXT  list the call chains that lead to the first function with TEXT in its name (innermost first)
  --depth N       frames per call chain (default 8)
The symbols come from the sidecar file run.json.syms.json that --unstable-presymbolicate writes.
"""
import bisect
import collections
import gzip
import json
import re
import sys


def option(name, default=None):
    if name in sys.argv:
        return sys.argv[sys.argv.index(name) + 1]
    return default


def short(name):
    name = re.sub(r"::h[0-9a-f]{16}$", "", name)
    while True:
        cut = re.sub(r"<[^<>]*>", "<>", name)
        if cut == name:
            break
        name = cut
    return name[:140]


def main():
    if len(sys.argv) < 2 or sys.argv[1] in ("-h", "--help"):
        print(__doc__)
        return 0
    path = sys.argv[1]
    thread_name = option("--thread")
    top = int(option("--top", "40"))
    under = option("--under")
    callers = option("--callers")
    depth = int(option("--depth", "8"))

    with gzip.open(path) as source:
        profile = json.load(source)
    with open(path.replace(".json.gz", ".json.syms.json"), encoding="utf-8") as source:
        sidecar = json.load(source)
    strings = sidecar["string_table"]
    tables = {}
    for lib in sidecar["data"]:
        table = sorted((entry["rva"], strings[entry["symbol"]]) for entry in lib["symbol_table"])
        tables[lib["debug_name"]] = ([row[0] for row in table], table)

    def symbol(lib, address):
        if lib not in tables:
            return f"[{lib}]"
        keys, table = tables[lib]
        index = bisect.bisect_right(keys, address) - 1
        return short(table[index][1]) if index >= 0 else f"[{lib}]"

    if thread_name:
        threads = [t for t in profile["threads"] if t["name"] == thread_name]
    else:
        # The main thread of the process that worked (a launcher in front of it has a main thread of its own).
        mains = [t for t in profile["threads"] if t.get("isMainThread")] or profile["threads"]
        threads = [max(mains, key=lambda t: len(t["samples"]["stack"]))]

    self_count, inclusive, chains = collections.Counter(), collections.Counter(), collections.Counter()
    total = 0
    for thread in threads:
        frame_table, stack_table, resources = thread["frameTable"], thread["stackTable"], thread["resourceTable"]
        names, stacks = {}, {}

        def frame_name(frame):
            if frame not in names:
                resource = thread["funcTable"]["resource"][frame_table["func"][frame]]
                lib = "?"
                if resource is not None and resource >= 0 and resources["lib"][resource] is not None:
                    lib = profile["libs"][resources["lib"][resource]]["debugName"]
                names[frame] = symbol(lib, frame_table["address"][frame])
            return names[frame]

        def frames_of(stack):
            out, at = [], stack
            while at is not None:
                if at in stacks:
                    out.extend(stacks[at])
                    break
                out.append(frame_name(stack_table["frame"][at]))
                at = stack_table["prefix"][at]
            stacks[stack] = out
            return out

        for stack in thread["samples"]["stack"]:
            if stack is None:
                continue
            frames = frames_of(stack)
            if under and not any(under in frame for frame in frames):
                continue
            total += 1
            self_count[frames[0]] += 1
            for frame in set(frames):
                inclusive[frame] += 1
            if callers:
                for index, frame in enumerate(frames):
                    if callers in frame:
                        chains[" <- ".join(frames[index:index + depth])] += 1
                        break

    print(f"thread {threads[0]['name'] if threads else '?'}: {total} samples")
    print("-- self")
    for name, count in self_count.most_common(top):
        print(f"{count:7d} {100 * count / max(total, 1):5.1f}%  {name}")
    print("-- inclusive")
    for name, count in inclusive.most_common(top):
        print(f"{count:7d} {100 * count / max(total, 1):5.1f}%  {name}")
    if callers:
        print(f"-- call chains to '{callers}'")
        for chain, count in chains.most_common(top):
            print(f"{count:7d} {100 * count / max(total, 1):5.1f}%  {chain}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
