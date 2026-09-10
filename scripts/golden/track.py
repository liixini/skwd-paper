"""Capture a burst of engine frames and print the x centre of the brightest blob per frame. Usage: track.py <project_dir> <out_dir> [frames]."""
import os, re, subprocess, sys, time
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import oracle, we_pkg


def centre(png):
    out = subprocess.run(['python3', os.path.join(HERE, 'measure.py'), png, 'blobs'], capture_output=True, text=True).stdout
    xs = re.findall(r'centre=\((\d+),(\d+)\)', out)
    return xs[1] if len(xs) > 1 else None


def main():
    proj, out_dir = sys.argv[1], sys.argv[2]
    frames = int(sys.argv[3]) if len(sys.argv) > 3 else 40
    os.makedirs(out_dir, exist_ok=True)
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
    raw, box, last = oracle.wait_ready(win['id'], out_dir, 'track', w, h, 150)
    t0 = time.time()
    for i in range(frames):
        png = os.path.join(out_dir, f'track_{i:03}.png')
        oracle.capture(win['id'], png)
        print(f'{time.time() - t0:6.2f} {centre(png)}', flush=True)


if __name__ == '__main__':
    main()
