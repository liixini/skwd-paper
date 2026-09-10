"""Wallpaper Engine scene.pkg reader/writer (header: u32 len + version, u32 count, entries of u32 len + name + u32 off + u32 size, then data)."""
import json, os, struct

WORKSHOP = os.path.expanduser('~/.local/share/Steam/steamapps/workshop/content/431960')
ASSETS = os.path.expanduser('~/.local/share/Steam/steamapps/common/wallpaper_engine/assets')


def entries(path):
    raw = open(path, 'rb').read()
    n = struct.unpack_from('<I', raw, 0)[0]
    version = raw[4:4 + n]
    p = 4 + n
    count = struct.unpack_from('<I', raw, p)[0]
    p += 4
    table = []
    for _ in range(count):
        ln = struct.unpack_from('<I', raw, p)[0]
        p += 4
        name = raw[p:p + ln].decode('utf-8')
        p += ln
        off, size = struct.unpack_from('<II', raw, p)
        p += 8
        table.append((name, off, size))
    return raw, p, table, version


def read_all(path):
    raw, base, table, version = entries(path)
    return {name: raw[base + off:base + off + size] for name, off, size in table}, version


def read(path, name):
    raw, base, table, _ = entries(path)
    for n, off, size in table:
        if n == name:
            return raw[base + off:base + off + size]
    return None


def pack(out_path, files, version=None):
    if version is None:
        version = b'PKGV0023'
    table = b''
    data = b''
    for name, content in files.items():
        nb = name.encode('utf-8')
        table += struct.pack('<I', len(nb)) + nb + struct.pack('<II', len(data), len(content))
        data += content
    open(out_path, 'wb').write(struct.pack('<I', len(version)) + version + struct.pack('<I', len(files)) + table + data)


def scene_json(project_dir):
    return json.loads(read(os.path.join(project_dir, 'scene.pkg'), 'scene.json'))


def canvas(project_dir):
    o = (scene_json(project_dir).get('general') or {}).get('orthogonalprojection') or {}
    return float(o.get('width', 1920) or 1920), float(o.get('height', 1080) or 1080)


if __name__ == '__main__':
    import sys
    path = sys.argv[1]
    if len(sys.argv) == 2:
        for n, off, size in entries(path)[2]:
            print(size, n)
    else:
        sys.stdout.buffer.write(read(path, sys.argv[2]) or b'')
