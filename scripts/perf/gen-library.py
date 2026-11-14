#!/usr/bin/env python3
"""Generates a library for performance measurements (docs/perf/README.md): folders with very many small,
low-quality WebP pictures (default: 100,000 in one folder plus 20,000 / 5,000 / 1,000 for size series,
about 1.5 GB).

Every picture is different (color field, shapes, its number in large digits), has a photo-like pixel
size and a file name and modification time as a camera would give it. The run is deterministic and
resumable: an existing file is kept. Nothing but the output folder is written. Needs Pillow and NumPy
(Arch: sudo pacman -S python-pillow python-numpy).

usage: gen-library.py --out DIR [--folders NAME=COUNT,...] [--jobs N] [--quality Q] [--force]
"""
import argparse
import os
import random
import sys
import time
from multiprocessing import Pool
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

DEFAULT_FOLDERS = "Flat-100k=100000,Flat-20k=20000,Flat-5k=5000,Flat-1k=1000"
# Pixel sizes of typical photos and screenshots, scaled to about one megapixel and below.
SIZES = [(1280, 853), (853, 1280), (1024, 768), (768, 1024), (1280, 720), (720, 1280), (960, 960), (1200, 800),
         (800, 1200), (1024, 576), (640, 480), (1366, 768)]
PREFIXES = ["IMG", "DSC", "PXL", "Photo", "Scan", "Screenshot", "holiday", "Trip", "cam", "P"]
EPOCH_START = 1420070400  # 2015-01-01
EPOCH_SPAN = 11 * 365 * 86400
_FONTS = {}


def font(size):
    if size not in _FONTS:
        _FONTS[size] = ImageFont.load_default(size=size)
    return _FONTS[size]


def file_name(folder_seed, index):
    rnd = random.Random(folder_seed * 1_000_003 + index)
    stamp = time.gmtime(EPOCH_START + rnd.randrange(EPOCH_SPAN))
    prefix = rnd.choice(PREFIXES)
    return f"{prefix}_{time.strftime('%Y%m%d_%H%M%S', stamp)}_{index:06d}.webp", time.mktime(stamp)


def render(folder_seed, index):
    rnd = random.Random(folder_seed * 7_000_003 + index)
    width, height = rnd.choice(SIZES)
    # A soft color field: a tiny random picture enlarged, like an out-of-focus photo.
    seed = np.random.default_rng(folder_seed * 31 + index)
    small = seed.integers(20, 236, size=(rnd.randint(3, 7), rnd.randint(3, 7), 3), dtype=np.uint8)
    image = Image.fromarray(small, "RGB").resize((width, height), Image.BICUBIC)
    draw = ImageDraw.Draw(image)
    for _ in range(rnd.randint(2, 5)):
        x0, y0 = rnd.randrange(width), rnd.randrange(height)
        x1, y1 = x0 + rnd.randint(width // 12, width // 3), y0 + rnd.randint(height // 12, height // 3)
        color = (rnd.randrange(256), rnd.randrange(256), rnd.randrange(256))
        if rnd.random() < 0.5:
            draw.ellipse((x0, y0, x1, y1), fill=color)
        else:
            draw.rectangle((x0, y0, x1, y1), fill=color)
    text = str(index)
    size = max(40, min(width, height) // 4)
    box = draw.textbbox((0, 0), text, font=font(size))
    x, y = (width - (box[2] - box[0])) // 2, (height - (box[3] - box[1])) // 2
    draw.text((x + 3, y + 3), text, font=font(size), fill=(0, 0, 0))
    draw.text((x, y), text, font=font(size), fill=(255, 255, 255))
    return image


def make_chunk(job):
    folder, folder_seed, start, stop, quality, force = job
    made = 0
    for index in range(start, stop):
        name, mtime = file_name(folder_seed, index)
        path = os.path.join(folder, name)
        if not force and os.path.exists(path):
            continue
        tmp = path + ".part"
        render(folder_seed, index).save(tmp, "WEBP", quality=quality, method=0)
        os.utime(tmp, (mtime, mtime))
        os.replace(tmp, path)
        made += 1
    return stop - start, made


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--folders", default=DEFAULT_FOLDERS)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--quality", type=int, default=10)
    parser.add_argument("--force", action="store_true", help="write existing files again")
    args = parser.parse_args()

    jobs = []
    total = 0
    for seed, spec in enumerate(args.folders.split(","), start=1):
        name, count = spec.split("=")
        count = int(count)
        folder = args.out / name
        folder.mkdir(parents=True, exist_ok=True)
        total += count
        for start in range(1, count + 1, 500):
            jobs.append((str(folder), seed, start, min(start + 500, count + 1), args.quality, args.force))

    began = time.time()
    done = made = 0
    with Pool(args.jobs) as pool:
        for seen, new in pool.imap_unordered(make_chunk, jobs):
            done += seen
            made += new
            if done % 5000 < 500:
                print(f"{done}/{total} files, {made} written, {time.time() - began:.0f} s", flush=True)
    print(f"done: {total} files in {args.out}, {made} written, {time.time() - began:.0f} s")


if __name__ == "__main__":
    sys.exit(main())
