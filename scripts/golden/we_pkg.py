"""Wallpaper Engine scene.pkg reader/writer (header: u32 len + version, u32 count, entries of u32 len + name + u32 off + u32 size, then data)."""
import json, os, struct

WORKSHOP = os.path.expanduser('~/.local/share/Steam/steamapps/workshop/content/431960')
ASSETS = os.path.expanduser('~/.local/share/Steam/steamapps/common/wallpaper_engine/assets')


MAX_PACKAGE_ENTRIES = 32768
MAX_PACKAGE_PATH_BYTES = 4096
MAX_PACKAGE_VERSION_BYTES = 32


def index(path):
    with open(path, 'rb') as stream:
        file_size = os.fstat(stream.fileno()).st_size

        def read_exact(size):
            if size > file_size - stream.tell():
                raise ValueError('truncated package index')
            value = stream.read(size)
            if len(value) != size:
                raise ValueError('package changed while reading index')
            return value

        def number():
            return struct.unpack('<I', read_exact(4))[0]

        length = number()
        if length > MAX_PACKAGE_VERSION_BYTES:
            raise ValueError('package version is too long')
        version = read_exact(length)
        if not version.startswith(b'PKGV'):
            raise ValueError('invalid package version')
        count = number()
        if count > MAX_PACKAGE_ENTRIES or count > (file_size - stream.tell()) // 12:
            raise ValueError('invalid package entry count')
        table = []
        for _ in range(count):
            length = number()
            if length > MAX_PACKAGE_PATH_BYTES or length > file_size - stream.tell() - 8:
                raise ValueError('invalid package path length')
            name = read_exact(length).decode('utf-8')
            offset, size = struct.unpack('<II', read_exact(8))
            table.append((name, offset, size))
        base = stream.tell()
        if any(offset + size > file_size - base for _, offset, size in table):
            raise ValueError('package entry is outside the file')
        return base, table, version


def entries(path):
    base, table, version = index(path)
    with open(path, 'rb') as stream:
        raw = stream.read()
    if any(base + offset + size > len(raw) for _, offset, size in table):
        raise ValueError('package changed after reading index')
    return raw, base, table, version


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
