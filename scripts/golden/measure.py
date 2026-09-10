"""measure.py <png> blobs | samples x,y[,x,y..]: bright-blob bounding boxes (canvas units assume a 1920x1080 canvas
scaled to the image) or pixel samples at canvas coordinates."""
import sys
from PIL import Image

im = Image.open(sys.argv[1]).convert('RGB')
W, H = im.size
px = im.load()
sx, sy = W / 1920, H / 1080
if sys.argv[2] == 'blobs':
    seen = set()
    blobs = []
    for y in range(H):
        for x in range(W):
            if (x, y) in seen or max(px[x, y]) < 40:
                continue
            stack = [(x, y)]
            seen.add((x, y))
            x0 = x1 = x
            y0 = y1 = y
            n = 0
            while stack:
                cx, cy = stack.pop()
                n += 1
                x0, x1, y0, y1 = min(x0, cx), max(x1, cx), min(y0, cy), max(y1, cy)
                for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                    if 0 <= nx < W and 0 <= ny < H and (nx, ny) not in seen and max(px[nx, ny]) >= 40:
                        seen.add((nx, ny))
                        stack.append((nx, ny))
            if n > 20:
                blobs.append((x0, y0, x1, y1, n, px[(x0 + x1) // 2, (y0 + y1) // 2]))
    for x0, y0, x1, y1, n, c in blobs:
        print(f'blob px=({x0},{y0})-({x1},{y1}) size={x1-x0+1}x{y1-y0+1} canvas: x={x0/sx:.0f}..{(x1+1)/sx:.0f} '
              f'y={y0/sy:.0f}..{(y1+1)/sy:.0f} w={(x1-x0+1)/sx:.0f} h={(y1-y0+1)/sy:.0f} '
              f'centre=({(x0+x1+1)/2/sx:.0f},{(y0+y1+1)/2/sy:.0f}) px_count={n} centre_rgb={c}')
else:
    vals = [float(v) for v in sys.argv[3].split(',')]
    for i in range(0, len(vals), 2):
        x, y = int(vals[i] * sx), int(vals[i + 1] * sy)
        print(f'canvas ({vals[i]:.0f},{vals[i+1]:.0f}) -> {px[x, y]}')
