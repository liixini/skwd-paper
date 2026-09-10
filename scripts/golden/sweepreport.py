"""Summarise an oracle sweep: distance distribution, worst scenes, and per-feature breakdown.
usage: sweepreport.py <out_dir> [baseline_csv]"""
import csv, json, os, statistics, sys

BANDS = [(0.05, 'near-identical'), (0.10, 'close'), (0.20, 'visible drift'), (1.01, 'clearly different')]


def band(distance):
    return next(name for limit, name in BANDS if distance < limit)


def features(baseline):
    if not os.path.isfile(baseline):
        return {}
    return {r['scene']: r for r in csv.DictReader(open(baseline))}


def main():
    out_dir = sys.argv[1]
    meta = features(sys.argv[2]) if len(sys.argv) > 2 else {}
    rows = json.load(open(os.path.join(out_dir, 'results.json')))
    ok = sorted(((r[2], r[0]) for r in rows if r[1] == 'ok' and r[2] is not None))
    bad = [(r[0], r[1]) for r in rows if r[1] != 'ok']
    if not ok:
        print('no measurements')
        return
    distances = [d for d, _ in ok]
    print(f'scenes measured {len(ok)}  failed {len(bad)}')
    print(f'distance mean {statistics.mean(distances):.4f}  median {statistics.median(distances):.4f}  p90 {sorted(distances)[int(len(distances) * 0.9)]:.4f}  max {max(distances):.4f}')
    counts = {name: 0 for _, name in BANDS}
    for d, _ in ok:
        counts[band(d)] += 1
    for _, name in BANDS:
        share = 100.0 * counts[name] / len(ok)
        print(f'  {name:20} {counts[name]:4}  {share:5.1f}%')
    print('\nworst 15')
    for d, sid in ok[-15:][::-1]:
        row = meta.get(sid, {})
        print(f'  {d:.4f}  {sid:12} particles={row.get("particles", "?"):>3} effects={row.get("chains", "?"):>2} animated={row.get("animated", "?")}')
    if bad:
        print('\nnot measured')
        for sid, status in bad:
            print(f'  {sid:12} {status}')
    groups = {}
    for d, sid in ok:
        row = meta.get(sid)
        if not row:
            continue
        key = ('particles' if int(row['particles'] or 0) > 2 else 'few/no particles', 'effects' if int(row['chains'] or 0) > 0 else 'no effects')
        groups.setdefault(key, []).append(d)
    print('\nby content')
    for key, values in sorted(groups.items(), key=lambda kv: -statistics.mean(kv[1])):
        print(f'  {key[0]:18} {key[1]:11} n={len(values):3} mean {statistics.mean(values):.4f} median {statistics.median(values):.4f}')


if __name__ == '__main__':
    main()
