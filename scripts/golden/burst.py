"""Capture N engine frames of each scene and print mean streak stats. Usage: burst.py <out_dir> <frames> <project_dir>..."""
import os, statistics, sys, time
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import oracle, we_pkg
from blobstats import stats


def main():
    out_dir, frames = sys.argv[1], int(sys.argv[2])
    os.makedirs(out_dir, exist_ok=True)
    for proj in sys.argv[3:]:
        sid = os.path.basename(proj.rstrip('/'))
        cw, ch = we_pkg.canvas(proj)
        w = 1280
        h = int(round(w * ch / cw))
        oracle.restart(proj, w, h)
        win = None
        for _ in range(90):
            time.sleep(1)
            win = oracle.we_window()
            if win:
                break
        oracle.park_windows()
        time.sleep(2)
        raw, box, last = oracle.wait_ready(win['id'], out_dir, sid, w, h, 150)
        time.sleep(4)
        rows = []
        for i in range(frames):
            png = os.path.join(out_dir, f'{sid}_{i:03}.png')
            oracle.capture(win['id'], png)
            rows.append(stats(png))
            time.sleep(0.3)
        counts = [r[0] for r in rows]
        print(f'{sid} frames {len(rows)} blobs mean {statistics.mean(counts):.1f} sd {statistics.pstdev(counts):.1f} medA {statistics.mean(r[1] for r in rows):.1f} p90A {statistics.mean(r[2] for r in rows):.1f}', flush=True)


if __name__ == '__main__':
    main()
