"""Streak statistics on a render: count, median/p90/max area, median peak. Usage: blobstats.py <png> [threshold]."""
import statistics, sys
from PIL import Image


def stats(path, threshold=84):
    im = Image.open(path).convert('L')
    w, h = im.size
    px = im.load()
    seen = bytearray(w * h)
    areas, peaks = [], []
    for y in range(h):
        for x in range(w):
            if px[x, y] <= threshold or seen[y * w + x]:
                continue
            stack = [(x, y)]
            seen[y * w + x] = 1
            area, peak = 0, 0
            while stack:
                cx, cy = stack.pop()
                area += 1
                peak = max(peak, px[cx, cy])
                for nx, ny in ((cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)):
                    if 0 <= nx < w and 0 <= ny < h and not seen[ny * w + nx] and px[nx, ny] > threshold:
                        seen[ny * w + nx] = 1
                        stack.append((nx, ny))
            areas.append(area)
            peaks.append(peak)
    if not areas:
        return (0, 0, 0, 0, 0)
    areas.sort()
    return (len(areas), int(statistics.median(areas)), areas[int(len(areas) * 0.9)], areas[-1], int(statistics.median(peaks)))


if __name__ == '__main__':
    print(stats(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 84))
