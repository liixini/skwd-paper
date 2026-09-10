"""ablate.py <scene_id> <out_root> <tag> [--first N] [--keep name,..] [--add name,..] [--drop name,..] [--drop-particles]:
repack a workshop scene with a filtered object list so the oracle can isolate which objects carry a difference.
Filters apply in the order first, keep, add, drop."""
import argparse, json, os, shutil, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import we_pkg


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('scene')
    ap.add_argument('out_root')
    ap.add_argument('tag')
    ap.add_argument('--first', type=int)
    ap.add_argument('--keep')
    ap.add_argument('--add')
    ap.add_argument('--drop')
    ap.add_argument('--drop-particles', action='store_true')
    args = ap.parse_args()
    src = f'{we_pkg.WORKSHOP}/{args.scene}'
    files, version = we_pkg.read_all(f'{src}/scene.pkg')
    scene = json.loads(files['scene.json'])
    every = scene['objects']
    objects = every[:args.first] if args.first else list(every)
    if args.keep:
        names = set(args.keep.split(','))
        objects = [o for o in objects if o.get('name') in names]
    if args.add:
        names = set(args.add.split(','))
        objects += [o for o in every if o.get('name') in names and o not in objects]
    if args.drop:
        names = set(args.drop.split(','))
        objects = [o for o in objects if o.get('name') not in names]
    if args.drop_particles:
        objects = [o for o in objects if not o.get('particle')]
    scene['objects'] = objects
    out = f'{args.out_root}/{args.scene}-{args.tag}'
    os.makedirs(out, exist_ok=True)
    shutil.copy(f'{src}/project.json', out)
    files['scene.json'] = json.dumps(scene).encode('utf-8')
    we_pkg.pack(f'{out}/scene.pkg', files, version)
    print(out, len(objects), 'objects')


if __name__ == '__main__':
    main()
