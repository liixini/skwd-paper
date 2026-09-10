"""Re-compare a swept scene against the engine capture across animation phases.
A single snapshot of an animated scene can differ only because the two are at different points in the
loop; the minimum over phases separates that from real content error.
usage: phasecheck.py <out_dir> <scene_id> [phases] [span_seconds]"""
import os, statistics, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import oracle, we_pkg
from PIL import Image


def ours_at(proj, out_dir, bw, bh, time0):
    frame = oracle.render_ours(proj, out_dir, bw, bh, time0)
    if not os.path.isfile(frame):
        return None
    im = Image.open(frame).convert('RGBA')
    ow, oh = im.size
    bg = Image.new('RGBA', im.size, (0, 0, 0, 255))
    bg.alpha_composite(im)
    png = f'{out_dir}.png'
    bg.convert('RGB').crop(((ow - bw) // 2, (oh - bh) // 2, (ow - bw) // 2 + bw, (oh - bh) // 2 + bh)).save(png)
    return png


def main():
    out_dir, sid = sys.argv[1], sys.argv[2]
    phases = int(sys.argv[3]) if len(sys.argv) > 3 else 12
    span = float(sys.argv[4]) if len(sys.argv) > 4 else 3.0
    we_png = f'{out_dir}/{sid}.we.png'
    bw, bh = Image.open(we_png).size
    proj = oracle.project_dir(sid)
    scores = []
    for step in range(phases):
        time0 = 1.0 + span * step / phases
        png = ours_at(proj, f'{out_dir}/{sid}.phase{step}', bw, bh, time0)
        if png is None:
            continue
        scores.append((oracle.compare(we_png, png), round(time0, 3), png))
    if not scores:
        print(sid, 'no renders')
        return
    scores.sort()
    best, worst = scores[0], scores[-1]
    print(f'{sid} phases={len(scores)} best={best[0]:.4f} at t={best[1]} median={statistics.median(s[0] for s in scores):.4f} worst={worst[0]:.4f}')
    subprocess.run(['magick', we_png, best[2], '+append', '-resize', '1800x', f'{out_dir}/{sid}.best.png'])


if __name__ == '__main__':
    main()
