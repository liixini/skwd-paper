"""oracle.py <out_dir> <scene_id_or_project_dir> [...]: capture the real Wallpaper Engine (Steam/Proton, windowed,
niri) and compare with skwd-wall-vk scene-dump. Window width 1280 (ORACLE_WIDTH), height follows the canvas aspect.
Env: SKWD_VK (binary), ORACLE_OUTPUT (niri monitor for the engine windows, DP-3), ORACLE_TIMEOUT (s, 150), ORACLE_SETTLE (s, 4), ORACLE_WARMUP (frames, 200), ORACLE_DT (s, 0.0667 =
the engine's configured 15 fps; per-frame integrators such as oscillateposition alias differently at other steps), ORACLE_WORKSPACE
(niri workspace, oracle), ORACLE_RESTART (1 = relaunch the engine per scene; steering a running instance stays grey).
Outputs <out_dir>/<id>.we.raw.png, .we.png, .ours/, .ours.png, .side.png and a merged results.json."""
import glob, json, os, shutil, subprocess, sys, time
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import we_pkg

VK = os.environ.get('SKWD_VK', os.path.join(HERE, '..', '..', 'target', 'release', 'skwd-wall-vk'))
SHOTS = os.path.expanduser('~/Pictures/Screenshots')
FRAME = (8, 34)
GREY = (179, 179, 179)


def project_dir(sid):
    return sid if os.path.isdir(sid) else f'{we_pkg.WORKSHOP}/{sid}'


def we_window():
    out = subprocess.run(['niri', 'msg', '--json', 'windows'], capture_output=True, text=True).stdout
    for w in json.loads(out or '[]'):
        if w.get('title') == 'skwd-oracle':
            return w
    return None


def launch_args(proj, w, h):
    return ['-control', 'openWallpaper', '-file', f'Z:{proj}/project.json', '-playInWindow', 'skwd-oracle',
            '-width', str(w - FRAME[0]), '-height', str(h - FRAME[1]), '-x', '0', '-y', '0', '-borderless']


def restart(proj, w, h):
    subprocess.run(['pkill', '-f', '[w]allpaper64.exe -language'], capture_output=True)
    subprocess.run(['pkill', '-f', '[l]auncher.exe -run wallpaper64.exe'], capture_output=True)
    subprocess.run(['pkill', '-f', '[s]team.exe.*wallpaper64.exe -control'], capture_output=True)
    for _ in range(30):
        time.sleep(1)
        if we_window() is None:
            break
    time.sleep(3)
    subprocess.Popen(['steam', '-applaunch', '431960'] + launch_args(proj, w, h), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def park_windows():
    ws = os.environ.get('ORACLE_WORKSPACE', 'oracle')
    monitor = os.environ.get('ORACLE_OUTPUT', 'DP-3')
    out = subprocess.run(['niri', 'msg', '--json', 'windows'], capture_output=True, text=True).stdout
    for w in json.loads(out or '[]'):
        if w.get('app_id') == 'steam_app_431960':
            subprocess.run(['niri', 'msg', 'action', 'move-window-to-workspace', '--window-id', str(w['id']), ws], capture_output=True)
    subprocess.run(['niri', 'msg', 'action', 'move-workspace-to-monitor', '--reference', ws, monitor], capture_output=True)


def capture(win_id, out_png):
    before = set(glob.glob(SHOTS + '/*.png'))
    subprocess.run(['niri', 'msg', 'action', 'screenshot-window', '--id', str(win_id), '-p', 'false'], capture_output=True)
    for _ in range(50):
        time.sleep(0.2)
        new = set(glob.glob(SHOTS + '/*.png')) - before
        if new:
            path = max(new)
            time.sleep(0.3)
            try:
                shutil.move(path, out_png)
                return True
            except OSError:
                before.add(path)
    return False


def classify(png, box):
    im = Image.open(png).convert('RGB')
    if box:
        im = im.crop(box)
    W, H = im.size
    px = im.load()
    n = grey = black = yellow = 0
    for y in range(0, H, 6):
        for x in range(0, W, 6):
            p = px[x, y]
            n += 1
            if all(abs(c - g) <= 8 for c, g in zip(p, GREY)):
                grey += 1
            elif p[0] < 12 and p[1] < 12 and p[2] < 12:
                black += 1
            elif p[0] > 200 and p[1] > 200 and p[2] < 60:
                yellow += 1
    if grey > 0.6 * n:
        return 'grey'
    if black > 0.85 * n and yellow > 0:
        return 'loader'
    if black > 0.97 * n:
        return 'black'
    return 'scene'


def grey_box(png, w, h):
    im = Image.open(png).convert('RGB')
    W, H = im.size
    px = im.load()

    def is_grey(p):
        return all(abs(c - g) <= 8 for c, g in zip(p, GREY))
    rows = [y for y in range(0, H, 2) if sum(1 for x in range(0, W, 8) if is_grey(px[x, y])) > w / 16]
    cols = [x for x in range(0, W, 2) if sum(1 for y in range(0, H, 8) if is_grey(px[x, y])) > h / 16]
    if not rows or not cols:
        return None
    x0, y0 = min(cols), min(rows)
    return (x0, y0, min(W, x0 + w), min(H, y0 + h))


def bright_box(png, w, h):
    im = Image.open(png).convert('L')
    W, H = im.size
    px = im.load()
    rows = [y for y in range(0, H, 2) if sum(1 for x in range(0, min(w, W), 8) if px[x, y] > 20) > w / 16]
    if not rows:
        return None
    y0 = min(rows)
    return (0, y0, min(w, W), min(H, y0 + h))


def wait_ready(win_id, out_dir, sid, w, h, timeout):
    box = None
    deadline = time.time() + timeout
    last = None
    got = None
    while time.time() < deadline:
        raw = f'{out_dir}/{sid}.we.raw.png'
        win = we_window()
        if win:
            win_id = win['id']
        if not capture(win_id, raw):
            last = 'capture-failed'
            time.sleep(2)
            continue
        got = raw
        if box is None:
            box = grey_box(raw, w, h)
        kind = classify(raw, box)
        if kind == 'scene':
            if box is None:
                box = bright_box(raw, w, h)
            return raw, box, last
        last = kind
        time.sleep(3)
    if got and last == 'black':
        return got, box or bright_box(got, w, h) or (0, 0, w, h), 'black-timeout'
    return None, box, last


def fit_pair(ours, engine):
    im = ours.convert('RGBA')
    bg = Image.new('RGBA', im.size, (0, 0, 0, 255))
    bg.alpha_composite(im)
    ours = bg.convert('RGB')
    engine = engine.convert('RGB')
    ow, oh = ours.size
    bw, bh = engine.size
    if (ow, oh) == (bw, bh):
        return ours, engine
    if ow * oh > bw * bh:
        return ours.resize((bw, bh), Image.LANCZOS), engine
    return ours, engine.resize((ow, oh), Image.LANCZOS)


def compare(a, b):
    r = subprocess.run(['magick', 'compare', '-metric', 'SSIM', a, b, 'null:'], capture_output=True, text=True)
    t = r.stderr.strip()
    try:
        return float(t.split('(')[1].split(')')[0])
    except Exception:
        return 1.0


def render_ours(proj, out_dir, bw, bh, settle):
    ours_dir = f'{out_dir}/ours'
    shutil.rmtree(ours_dir, ignore_errors=True)
    subprocess.run([VK, 'scene-dump', proj, '--out', ours_dir, '--size', f'{bw}x{bh}', '--frames', '1',
                    '--warmup', os.environ.get('ORACLE_WARMUP', '200'), '--dt', os.environ.get('ORACLE_DT', '0.0667'), '--time0', str(settle),
                    '--daytime', '0.5'], capture_output=True, timeout=300)
    return f'{ours_dir}/frame_0000.png'


def main():
    out_dir = sys.argv[1]
    os.makedirs(out_dir, exist_ok=True)
    results = []
    timeout = float(os.environ.get('ORACLE_TIMEOUT', '150'))
    settle = float(os.environ.get('ORACLE_SETTLE', '4'))
    skip = done_ok(out_dir)
    for arg in sys.argv[2:]:
        proj = project_dir(arg.rstrip('/'))
        sid = os.path.basename(arg.rstrip('/'))
        if sid in skip:
            print(sid, 'already measured', flush=True)
            continue
        cw, ch = we_pkg.canvas(proj)
        w = int(os.environ.get('ORACLE_WIDTH', '1280'))
        h = int(round(w * ch / cw))
        started = time.time()
        if os.environ.get('ORACLE_RESTART', '1') == '1' or we_window() is None:
            restart(proj, w, h)
        win = None
        for _ in range(90):
            time.sleep(1)
            win = we_window()
            if win:
                break
        park_windows()
        if not win:
            results.append((sid, 'no-window', None))
            save(out_dir, results)
            print(sid, 'no window', flush=True)
            continue
        time.sleep(2)
        raw, box, last = wait_ready(win['id'], out_dir, sid, w, h, timeout)
        if raw is None:
            results.append((sid, f'not-ready:{last}', None))
            save(out_dir, results)
            print(sid, 'not ready', last, flush=True)
            continue
        time.sleep(settle)
        capture(win['id'], raw)
        raw_im = Image.open(raw)
        if raw_im.size != (w, h):
            win = we_window() or win
            subprocess.run(['niri', 'msg', 'action', 'set-window-width', '--id', str(win['id']), str(w)], capture_output=True)
            subprocess.run(['niri', 'msg', 'action', 'set-window-height', '--id', str(win['id']), str(h)], capture_output=True)
            time.sleep(3)
            capture(win['id'], raw)
            raw_im = Image.open(raw)
            print(sid, 'resized window to', raw_im.size, flush=True)
        if raw_im.size == (w, h) or box is None:
            box = (0, 0, raw_im.size[0], raw_im.size[1])
        we_png = f'{out_dir}/{sid}.we.png'
        raw_im.crop(box).save(we_png)
        bw, bh = Image.open(we_png).size
        frame = render_ours(proj, f'{out_dir}/{sid}', bw, bh, settle)
        ours_png = f'{out_dir}/{sid}.ours.png'
        if not os.path.isfile(frame):
            results.append((sid, 'ours-failed', None))
            save(out_dir, results)
            print(sid, 'ours failed', flush=True)
            continue
        ours_im, we_im = fit_pair(Image.open(frame), Image.open(we_png))
        ours_im.save(ours_png)
        we_im.save(we_png)
        bw, bh = we_im.size
        d = compare(we_png, ours_png)
        subprocess.run(['magick', we_png, ours_png, '+append', '-resize', '1800x', f'{out_dir}/{sid}.side.png'])
        elapsed = round(time.time() - started, 1)
        results.append((sid, 'ok', d, elapsed))
        save(out_dir, results)
        print(sid, f'window {w}x{h} box {bw}x{bh} distance={d:.4f} seconds={elapsed}', flush=True)

def save(out_dir, results):
    path = f'{out_dir}/results.json'
    previous = json.load(open(path)) if os.path.isfile(path) else []
    merged = {r[0]: r for r in previous}
    for r in results:
        merged[r[0]] = r
    json.dump(list(merged.values()), open(path, 'w'))


def done_ok(out_dir):
    path = f'{out_dir}/results.json'
    if os.environ.get('ORACLE_SKIP_DONE') != '1' or not os.path.isfile(path):
        return set()
    return {r[0] for r in json.load(open(path)) if r[1] == 'ok'}


if __name__ == '__main__':
    main()
