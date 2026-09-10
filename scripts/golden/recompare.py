"""Recompute ours-vs-engine distances from captures already on disk, without relaunching the engine.
The engine capture is authoritative and reused; only our render is redone, then both are brought to a
common size (the smaller of the two) before comparing.
usage: recompare.py <out_dir> [scene_id ...]"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import oracle
from PIL import Image


def main():
    out_dir = sys.argv[1]
    path = os.path.join(out_dir, 'results.json')
    rows = json.load(open(path))
    wanted = set(sys.argv[2:])
    updated = {r[0]: r for r in rows}
    for row in rows:
        sid = row[0]
        if wanted and sid not in wanted:
            continue
        we_png = os.path.join(out_dir, f'{sid}.we.png')
        if not os.path.isfile(we_png):
            continue
        engine = Image.open(we_png)
        bw, bh = engine.size
        frame = oracle.render_ours(oracle.project_dir(sid), os.path.join(out_dir, sid), bw, bh, 4.0)
        if not os.path.isfile(frame):
            print(sid, 'ours failed', flush=True)
            continue
        ours_im, we_im = oracle.fit_pair(Image.open(frame), engine)
        ours_png = os.path.join(out_dir, f'{sid}.ours.png')
        fit_png = os.path.join(out_dir, f'{sid}.we.fit.png')
        ours_im.save(ours_png)
        we_im.save(fit_png)
        d = oracle.compare(fit_png, ours_png)
        before = row[2] if len(row) > 2 and row[2] is not None else float('nan')
        print(f'{sid} {before:.4f} -> {d:.4f}  ours {ours_im.size[0]}x{ours_im.size[1]}', flush=True)
        updated[sid] = [sid, 'ok', d, row[3] if len(row) > 3 else None]
        json.dump(list(updated.values()), open(path, 'w'))


if __name__ == '__main__':
    main()
