"""Render N frames of each probe with skwd-wall-vk and print mean streak stats. Usage: ourburst.py <out_dir> <frames> <project_dir>..."""
import glob, os, statistics, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from blobstats import stats
VK = os.environ.get('SKWD_VK', os.path.join(HERE, '..', '..', 'target', 'release', 'skwd-wall-vk'))


def main():
    out_dir, frames = sys.argv[1], int(sys.argv[2])
    for proj in sys.argv[3:]:
        sid = os.path.basename(proj.rstrip('/'))
        rows = []
        for i in range(frames):
            out = os.path.join(out_dir, sid, str(i))
            subprocess.run(['rm', '-rf', out])
            subprocess.run([VK, 'scene-dump', proj, '--out', out, '--size', '1280x720', '--frames', '1', '--warmup', str(240 + 15 * i),
                            '--dt', os.environ.get('OUR_DT', '0.0667'), '--time0', '4', '--daytime', '0.5'], capture_output=True, timeout=300)
            rows.extend(stats(png) for png in sorted(glob.glob(os.path.join(out, 'frame_*.png'))))
        counts = [r[0] for r in rows]
        print(f'{sid} frames {len(rows)} blobs mean {statistics.mean(counts):.1f} sd {statistics.pstdev(counts):.1f} medA {statistics.mean(r[1] for r in rows):.1f} p90A {statistics.mean(r[2] for r in rows):.1f}', flush=True)


if __name__ == '__main__':
    main()
