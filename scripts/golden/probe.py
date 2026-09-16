"""probe.py <out_root> <probe> [...] | all: write synthetic probe projects (loose files + packed scene.pkg) for the
engine oracle. Probes embed preset/effect files because a loose project cannot see assets/presets."""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import we_pkg

CAMERA = {'center': '0 0 -1', 'eye': '0 0 0', 'up': '0 1 0'}
WHITE_MODEL = {'material': 'materials/white.json', 'autosize': False, 'fullscreen': False, 'passthrough': False, 'puppet': None}
WHITE_MAT = {'passes': [{'alphawriting': 'default', 'blending': 'translucent', 'combos': {}, 'cullmode': 'nocull',
                         'depthtest': 'disabled', 'depthwrite': 'disabled', 'shader': 'genericimage2', 'textures': ['util/white']}]}
WHITE_MAT_V2 = {'passes': [{'alphawriting': 'default', 'blending': 'translucent', 'combos': {'VERSION': 2}, 'cullmode': 'nocull',
                            'depthtest': 'disabled', 'depthwrite': 'disabled', 'shader': 'genericimage2', 'textures': ['util/white']}]}
PARTICLE_MAT = {'passes': [{'blending': 'translucent', 'combos': {}, 'cullmode': 'nocull', 'depthtest': 'disabled',
                            'depthwrite': 'disabled', 'shader': 'genericparticle', 'textures': ['util/white']}]}


def general(w=1920, h=1080, **extra):
    g = {'ambientcolor': '0.2 0.2 0.2', 'bloom': False, 'camerafade': False, 'cameraparallax': False, 'camerashake': False,
         'clearcolor': '0 0 0', 'clearenabled': True, 'farz': 10000.0, 'fov': 50.0, 'hdr': False, 'nearz': 0.01,
         'orthogonalprojection': {'auto': False, 'height': h, 'width': w}, 'skylightcolor': '0.3 0.3 0.3', 'zoom': 1.0}
    g.update(extra)
    return g


def write(root, name, files, title=None):
    folder = f'{root}/{name}'
    os.makedirs(folder, exist_ok=True)
    blobs = {}
    for path, obj in files.items():
        data = obj if isinstance(obj, (bytes, bytearray)) else json.dumps(obj, indent=1).encode('utf-8')
        full = f'{folder}/{path}'
        os.makedirs(os.path.dirname(full), exist_ok=True)
        open(full, 'wb').write(data)
        blobs[path] = data
    json.dump({'file': 'scene.json', 'title': title or name, 'type': 'scene', 'version': 0, 'general': {'properties': {}}},
              open(f'{folder}/project.json', 'w'), indent=1)
    we_pkg.pack(f'{folder}/scene.pkg', blobs)
    return folder


def scene(objects, **general_extra):
    return {'camera': CAMERA, 'general': general(**general_extra), 'objects': objects}


def image_layer(oid, name, origin, size, color='1 1 1', brightness=1.0, alpha=1.0, scale='1 1 1', model='models/white.json', **extra):
    o = {'id': oid, 'name': name, 'image': model, 'origin': origin, 'size': size, 'scale': scale, 'angles': '0 0 0',
         'color': color, 'brightness': brightness, 'alpha': alpha, 'visible': True, 'parallaxDepth': '1 1',
         'copybackground': False, 'ledsource': False, 'perspective': False, 'solid': True, 'castshadow': False,
         'clampuvs': True, 'disablepropagation': False, 'locktransforms': False}
    o.update(extra)
    return o


def particle_object(oid, name, origin, particle, scale='1 1 1', override=None, angles='0 0 0'):
    return {'id': oid, 'name': name, 'origin': origin, 'scale': scale, 'angles': angles, 'particle': particle,
            'visible': True, 'instanceoverride': override or {'id': oid + 1000}, 'parallaxDepth': '1 1'}


def particle_doc(size, emitter_origin='0 0 0', flags=None, starttime=2, renderer=None, **extra):
    d = {'controlpoint': [{'flags': 0, 'id': i, 'offset': '0 0 0'} for i in range(8)],
         'emitter': [{'distancemax': '0 0 0', 'id': 7, 'name': 'boxrandom', 'origin': emitter_origin, 'rate': 5}],
         'initializer': [{'id': 2, 'max': 1000, 'min': 1000, 'name': 'lifetimerandom'},
                         {'id': 3, 'max': size, 'min': size, 'name': 'sizerandom'}],
         'material': 'materials/probe_particle.json', 'maxcount': 1, 'operator': [],
         'renderer': [renderer or {'id': 1, 'name': 'sprite'}], 'starttime': starttime}
    if flags is not None:
        d['flags'] = flags
    d.update(extra)
    return d


def text_layer(oid, name, origin, text, pointsize, size='600 200', halign='center', valign='center', padding=0,
               color='1 1 1', font='fonts/NotoSans-Regular.ttf', anchor='none', **extra):
    o = {'anchor': anchor, 'backgroundbrightness': 1.0, 'backgroundcolor': '0 0 0', 'blockalign': False, 'brightness': 1.0,
         'castshadow': False, 'clampuvs': True, 'color': color, 'depthtest': 'enabled', 'disablepropagation': False,
         'font': font, 'horizontalalign': halign, 'id': oid, 'limitrows': False, 'limituseellipsis': False,
         'limitwidth': False, 'maxrows': 1, 'maxwidth': 500.0, 'name': name, 'opaquebackground': False, 'origin': origin,
         'padding': padding, 'pointsize': pointsize, 'size': size, 'text': text, 'verticalalign': valign, 'visible': True,
         'angles': '0 0 0', 'scale': '1 1 1', 'alpha': 1.0, 'parallaxDepth': '1 1'}
    o.update(extra)
    return o


def preset_files(group, name):
    base = f'{we_pkg.ASSETS}/presets/{group}'
    return {f'particles/presets/{name}.json': json.load(open(f'{base}/particles/presets/{name}.json')),
            f'materials/presets/{name}.json': json.load(open(f'{base}/materials/presets/{name}.json'))}


def pkg_files(pkg_path, names):
    out = {}
    for n in names:
        data = we_pkg.read(pkg_path, n)
        out[n] = json.loads(data) if n.endswith('.json') else data
    return out


def white_probe_files(objects, **general_extra):
    return {'models/white.json': WHITE_MODEL, 'materials/white.json': WHITE_MAT_V2, 'scene.json': scene(objects, **general_extra)}


def probe_pscale(root):
    return write(root, 'probe_pscale', {
        'materials/probe_particle.json': PARTICLE_MAT,
        'particles/p100.json': particle_doc(100), 'particles/p100off.json': particle_doc(100, '100 0 0'),
        'scene.json': scene([particle_object(1, 'unit', '300 540 0', 'particles/p100.json'),
                             particle_object(3, 'triple', '1100 540 0', 'particles/p100off.json', scale='3 3 1')])})


def probe_bright2(root):
    gi3 = {'passes': [{'alphawriting': 'default', 'blending': 'translucent', 'combos': {'LIGHTING': 0, 'REFLECTION': 0, 'VERSION': 2},
                       'cullmode': 'nocull', 'depthtest': 'disabled', 'depthwrite': 'disabled', 'shader': 'genericimage3', 'textures': ['util/white']}]}
    files = {'models/white.json': WHITE_MODEL, 'materials/white.json': WHITE_MAT,
             'models/gi3.json': dict(WHITE_MODEL, material='materials/gi3.json'), 'materials/gi3.json': gi3,
             'models/gi2v.json': dict(WHITE_MODEL, material='materials/gi2v.json'), 'materials/gi2v.json': WHITE_MAT_V2}
    objs = [image_layer(1, 'gi2-red', '200 300 0', '200 200', color='1 0 0'),
            image_layer(2, 'gi3v2-red', '500 300 0', '200 200', color='1 0 0', model='models/gi3.json'),
            image_layer(4, 'gi2v2-red', '1100 300 0', '200 200', color='1 0 0', model='models/gi2v.json'),
            image_layer(5, 'gi3v2-alpha', '1400 300 0', '200 200', alpha=0.5, model='models/gi3.json'),
            image_layer(6, 'gi3v2-bright', '1700 300 0', '200 200', brightness=0.5, model='models/gi3.json'),
            image_layer(7, 'gi2-alpha', '200 780 0', '200 200', alpha=0.5),
            image_layer(9, 'gi3v2-grey-double', '800 780 0', '200 200', color='0.5 0.5 0.5', brightness=2.0, model='models/gi3.json'),
            image_layer(12, 'gi3v2-colorbright', '1700 780 0', '200 200', color='1 0.5 0', brightness=1.5, model='models/gi3.json')]
    files['scene.json'] = scene(objs)
    return write(root, 'probe_bright2', files)


def probe_perspective(root):
    def pdoc(size, origin, flags):
        return particle_doc(size, origin, flags=flags)
    files = {'materials/probe_particle.json': PARTICLE_MAT,
             'particles/o0.json': pdoc(100, '0 0 0', 0), 'particles/p0.json': pdoc(100, '0 0 0', 4),
             'particles/pfront.json': pdoc(100, '0 0 300', 4), 'particles/pback.json': pdoc(100, '0 0 -300', 4),
             'particles/ofront.json': pdoc(100, '0 0 300', 0)}
    files['scene.json'] = scene([particle_object(1, 'ortho z0', '200 540 0', 'particles/o0.json'),
                                 particle_object(3, 'persp z0', '500 540 0', 'particles/p0.json'),
                                 particle_object(5, 'persp z+300', '900 540 0', 'particles/pfront.json'),
                                 particle_object(7, 'persp z-300', '1300 540 0', 'particles/pback.json'),
                                 particle_object(9, 'ortho z+300', '1700 540 0', 'particles/ofront.json')])
    return write(root, 'probe_persp', files)


def probe_color(root):
    doc = particle_doc(24, starttime=3, maxcount=120)
    doc['emitter'] = [{'distancemax': '900 450 0', 'id': 7, 'name': 'boxrandom', 'origin': '0 0 0', 'rate': 100}]
    doc['initializer'].append({'id': 4, 'max': '0 0 255', 'min': '255 0 0', 'name': 'colorrandom'})
    return write(root, 'probe_color', {'materials/probe_particle.json': PARTICLE_MAT, 'particles/c.json': doc,
                                       'scene.json': scene([particle_object(1, 'colors', '960 540 0', 'particles/c.json')])})


def probe_sign_values(root):
    files = {'materials/probe_particle.json': PARTICLE_MAT}
    objs = []
    for i, (tag, sign) in enumerate((('s1', '1 0 0'), ('s10', '10 0 0'), ('s2', '2 0 0'), ('shalf', '0.5 0 0'), ('sneg1', '-1 0 0'), ('sneg10', '-10 0 0'))):
        doc = particle_doc(60, maxcount=8)
        doc['emitter'] = [{'directions': '1 0 0', 'distancemax': 120, 'distancemin': 120, 'id': 7, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 30, 'sign': sign}]
        files[f'particles/{tag}.json'] = doc
        objs.append(particle_object(10 + i, tag, f'{300 + i * 300} {300 if i % 2 else 780} 0', f'particles/{tag}.json'))
    files['scene.json'] = scene(objs)
    return write(root, 'probe_sign_values', files)


def probe_text1(root):
    return write(root, 'probe_text1', {'scene.json': scene([
        text_layer(1, 'H48', '300 540 0', 'H', 48), text_layer(2, 'H96', '700 540 0', 'H', 96),
        text_layer(3, 'HHH48', '1100 540 0', 'H H', 48),
        text_layer(4, 'lefttop', '1500 540 0', 'H', 48, size='300 300', halign='left', valign='top'),
        text_layer(6, 'rightbottom', '300 200 0', 'H', 48, size='300 300', halign='right', valign='bottom'),
        text_layer(7, 'red-half', '700 200 0', 'H', 48, color='1 0 0', alpha=0.5),
        text_layer(8, 'H48-880', '1100 880 0', 'H', 48, size='300 300')])})


def probe_text_lines(root):
    objs = []
    oid = 1
    for column, text in enumerate(['H', 'H\nH\nH', 'H\nH\nH\nH\nH']):
        for row, valign in enumerate(['top', 'center', 'bottom']):
            objs.append(text_layer(oid, f'{valign}-{column}', f'{300 + column * 500} {880 - row * 320} 0', text, 48,
                                   size='300 300', halign='left', valign=valign))
            oid += 1
    return write(root, 'probe_text_lines', {'scene.json': scene(objs)})


def probe_text_lines2(root):
    objs = []
    oid = 1
    for column, (lines, box) in enumerate([(1, '300 300'), (3, '300 300'), (3, '300 900'), (1, '300 900'), (3, '900 900'), (5, '300 300')]):
        for row, valign in enumerate(['top', 'center', 'bottom']):
            text = '\n'.join(['H'] * lines)
            objs.append(text_layer(oid, f'{valign}-{lines}-{box.split()[1]}', f'{100 + column * 300} {(3 - row) * 270} 0', text, 24,
                                   size=box, halign='left', valign=valign))
            oid += 1
    return write(root, 'probe_text_lines2', {'scene.json': scene(objs)})


def probe_text_lines3(root):
    objs = [image_layer(1, 'backdrop', '960 540 0', '1920 1080', color='0.5 0.5 0.5')]
    objs.append(text_layer(2, 'top-3', '200 900 0', 'H\nH\nH', 24, size='300 600', halign='left', valign='top'))
    objs.append(text_layer(3, 'top-5', '700 950 0', 'H\nH\nH\nH\nH', 24, size='300 600', halign='left', valign='top'))
    objs.append(text_layer(4, 'center-3', '1200 540 0', 'H\nH\nH', 24, size='300 600', halign='left', valign='center'))
    objs.append(text_layer(5, 'bottom-3', '1700 200 0', 'H\nH\nH', 24, size='300 600', halign='left', valign='bottom'))
    return write(root, 'probe_text_lines3', white_probe_files(objs))


def probe_text_bearing2(root):
    objs = []
    oid = 1
    for i, t in enumerate(['H', 'L', 'J', 'T', 'i', 'HL']):
        objs.append(text_layer(oid, f'r-{t}', f'{350 + i * 300} 850 0', t, 48, halign='right', valign='top')); oid += 1
        objs.append(text_layer(oid, f'l-{t}', f'{50 + i * 300} 550 0', t, 48, halign='left', valign='top')); oid += 1
        objs.append(text_layer(oid, f'c-{t}', f'{200 + i * 300} 250 0', t, 48, halign='center', valign='top')); oid += 1
    return write(root, 'probe_text_bearing2', {'scene.json': scene(objs)})


def probe_bloom(root):
    objs = [image_layer(1, 'white', '480 540 0', '200 200'), image_layer(2, 'grey', '1440 540 0', '200 200', color='0.4 0.4 0.4'),
            image_layer(3, 'red', '960 800 0', '120 120', color='1 0 0'), image_layer(4, 'thin', '960 250 0', '400 20')]
    return write(root, 'probe_bloom', white_probe_files(objs, bloom=True, bloomstrength=2.0, bloomthreshold=0.5, bloomtint='1 1 1'))


def probe_pulse(root):
    pkg = f'{we_pkg.WORKSHOP}/1444077782/scene.pkg'
    files = pkg_files(pkg, ['effects/pulse/effect.json', 'materials/effects/pulse.json', 'shaders/effects/pulse.vert', 'shaders/effects/pulse.frag'])
    tints = {'ui_editor_properties_tint_high': '1 0.44313725490196076 0.28627450980392155', 'ui_editor_properties_tint_low': '0.8745098039215686 0.996078431372549 1'}

    def layer(oid, name, origin, color, phase, extra=None):
        consts = dict(tints, ui_editor_properties_pulse_speed=0.0, ui_editor_properties_pulse_phase=phase)
        consts.update(extra or {})
        o = image_layer(oid, name, origin, '240 240', color=color)
        o['effects'] = [{'file': 'effects/pulse/effect.json', 'id': 100 + oid, 'name': '', 'passes': [{'constantshadervalues': consts, 'id': 200 + oid}], 'visible': True}]
        return o
    objs = [layer(1, 'white-high', '200 540 0', '1 1 1', 1.5708), layer(2, 'white-low', '500 540 0', '1 1 1', 4.7124),
            layer(3, 'grey-high', '800 540 0', '0.5 0.5 0.5', 1.5708), layer(4, 'grey-low', '1100 540 0', '0.5 0.5 0.5', 4.7124),
            layer(5, 'grey-mid', '1400 540 0', '0.5 0.5 0.5', 0.0),
            layer(6, 'grey-default', '1700 540 0', '0.5 0.5 0.5', 1.5708, {'ui_editor_properties_tint_high': '1 1 1', 'ui_editor_properties_tint_low': '1 1 1'})]
    files.update(white_probe_files(objs))
    return write(root, 'probe_pulse', files)


def probe_shafts(root):
    outs = []
    for tag, over in (('norate', {'id': None, 'size': 2.8, 'speed': 0}), ('rate001', {'id': None, 'rate': 0.01, 'size': 2.8, 'speed': 0})):
        files = preset_files('lightshafts', 'light_shafts_0')
        files['scene.json'] = scene([particle_object(1, 'Light shafts 0', '960 540 0', 'particles/presets/light_shafts_0.json',
                                                     scale='0.900 2.900 1.000', override=over, angles='0.000 0.000 0.335')])
        outs.append(write(root, f'probe_shafts_{tag}', files))
    return outs


DOWNPOUR_FILES = ['particles/workshop/2251009624/Rain_down_pour_x_copy1.json', 'materials/workshop/2251009624/particle/raindownpour_1.json',
                  'materials/workshop/2251009624/particle/Rain.tex', 'materials/workshop/2251009624/particle/Rain refraction.tex']


def downpour_probe(root, name, patch):
    files = pkg_files(f'{we_pkg.WORKSHOP}/3597803619/scene.pkg', DOWNPOUR_FILES)
    doc = files[DOWNPOUR_FILES[0]]
    patch(doc)
    files.update(white_probe_files([image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'),
                                    particle_object(2, 'Rain downpour', '699.0957 622.87366 0', DOWNPOUR_FILES[0], scale='1.74781 1.74781 2.21329')]))
    return write(root, name, files)


def single_particle(doc):
    doc['emitter'][0].update({'directions': '1 1 0', 'distancemax': 0, 'distancemin': 0, 'rate': 0.5, 'instantaneous': 0})
    doc['maxcount'] = 1
    doc['initializer'] = [{'id': 2, 'max': 1000, 'min': 1000, 'name': 'lifetimerandom'}, {'id': 3, 'max': 100, 'min': 100, 'name': 'sizerandom'},
                          {'id': 4, 'max': 0.7, 'min': 0.7, 'name': 'alpharandom'}]
    doc['operator'] = []
    doc['starttime'] = 2


def probe_downpour(root):
    return [downpour_probe(root, 'probe_downpour', lambda d: None),
            downpour_probe(root, 'probe_dp_single', single_particle),
            downpour_probe(root, 'probe_dps_sysflags0', lambda d: (single_particle(d), d.update({'flags': 0}))),
            downpour_probe(root, 'probe_dps_noorient', lambda d: (single_particle(d), d['renderer'][0].pop('orientation', None)))]


def probe_dp_white(root):
    outs = []
    for tag, flags, orient in (('w7', 7, 'screen'), ('w5', 5, 'screen'), ('w7none', 7, None), ('w0', 0, 'screen')):
        doc = particle_doc(100, flags=flags, renderer={'flags': 1, 'id': 1, 'name': 'sprite'})
        doc['emitter'] = [{'directions': '1 1 0', 'distancemax': 0, 'distancemin': 0, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 0.5, 'sign': '10 10 10'}]
        if orient:
            doc['renderer'][0]['orientation'] = orient
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
                 'scene.json': scene([particle_object(2, 'white single', '960 540 0', 'particles/p.json')])}
        outs.append(write(root, f'probe_dpw_{tag}', files))
    return outs


def probe_dp_material(root):
    outs = []
    for tag, textures, combos, blending in (('refwhite', ['util/white', 'workshop/2251009624/particle/Rain refraction'], {'REFRACT': 1}, 'additive'),
                                            ('rainnoref', ['workshop/2251009624/particle/Rain'], {}, 'additive'),
                                            ('rainref', ['workshop/2251009624/particle/Rain', 'workshop/2251009624/particle/Rain refraction'], {'REFRACT': 1}, 'additive')):
        files = pkg_files(f'{we_pkg.WORKSHOP}/3597803619/scene.pkg', DOWNPOUR_FILES)
        doc = files[DOWNPOUR_FILES[0]]
        single_particle(doc)
        files['materials/workshop/2251009624/particle/raindownpour_1.json'] = {'passes': [{
            'blending': blending, 'combos': combos, 'constantshadervalues': {'ui_editor_properties_overbright': 1, 'ui_editor_properties_refract_amount': 1},
            'cullmode': 'nocull', 'depthtest': 'disabled', 'depthwrite': 'disabled', 'shader': 'genericparticle', 'textures': textures}]}
        files.update(white_probe_files([image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'),
                                        particle_object(2, 'Rain single', '960 540 0', DOWNPOUR_FILES[0])]))
        outs.append(write(root, f'probe_dpm_{tag}', files))
    return outs


def probe_sphere_dist(root):
    doc = particle_doc(60, starttime=6, maxcount=120)
    doc['emitter'] = [{'directions': '1 1 0', 'distancemax': 2000, 'distancemin': 1, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 30, 'sign': '10 10 10'}]
    files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/d.json': doc,
             'scene.json': scene([particle_object(1, 'dist', '0 0 0', 'particles/d.json', scale='0.5 0.5 1')])}
    return write(root, 'probe_sphere_dist', files)


def probe_rate(root):
    outs = []
    for rate in (250, 2500, 25000):
        doc = particle_doc(6, starttime=4, maxcount=100000)
        doc['emitter'] = [{'directions': '1 1 0', 'distancemax': 500, 'distancemin': 0, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': rate}]
        doc['initializer'][0] = {'id': 2, 'max': 1, 'min': 1, 'name': 'lifetimerandom'}
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/r.json': doc,
                 'scene.json': scene([particle_object(1, f'rate{rate}', '960 540 0', 'particles/r.json')])}
        outs.append(write(root, f'probe_rate_{rate}', files))
    return outs


def probe_dp_motion(root):
    outs = []
    for tag, rflags, orient in (('mv1', 1, 'screen'), ('mv0', 0, 'screen'), ('mv1none', 1, None)):
        files = pkg_files(f'{we_pkg.WORKSHOP}/3597803619/scene.pkg', DOWNPOUR_FILES)
        doc = files[DOWNPOUR_FILES[0]]
        single_particle(doc)
        doc['initializer'].append({'id': 5, 'max': '0 -500 0', 'min': '0 -500 0', 'name': 'velocityrandom'})
        doc['initializer'][0] = {'id': 2, 'max': 2.0, 'min': 2.0, 'name': 'lifetimerandom'}
        doc['emitter'][0]['rate'] = 0.5
        doc['maxcount'] = 1
        doc['operator'] = [{'gravity': '0 0 0', 'id': 7, 'name': 'movement'}]
        doc['starttime'] = 0
        doc['renderer'] = [{'flags': rflags, 'id': 1, 'name': 'sprite'}]
        if orient:
            doc['renderer'][0]['orientation'] = orient
        files.update(white_probe_files([image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'),
                                        particle_object(2, 'Rain moving', '960 900 0', DOWNPOUR_FILES[0])]))
        outs.append(write(root, f'probe_dpmv_{tag}', files))
    return outs


def probe_downpour_flat(root):
    return [downpour_probe(root, 'probe_downpour_flatz', lambda d: d['emitter'][0].update({'directions': '1 1 0'})),
            downpour_probe(root, 'probe_downpour_noperspective', lambda d: d.update({'flags': 3}))]


def probe_downpour_params(root):
    def novel(d):
        d['initializer'] = [i for i in d['initializer'] if i['name'] != 'velocityrandom']
    def longlife(d):
        for i in d['initializer']:
            if i['name'] == 'lifetimerandom':
                i['min'] = i['max'] = 10.0
    def noosc(d):
        d['operator'] = [o for o in d['operator'] if o['name'] != 'oscillateposition']
    def noexp(d):
        for i in d['initializer']:
            i.pop('exponent', None)
    def lowrate(d):
        d['emitter'][0].update({'rate': 2500, 'instantaneous': 0})
    return [downpour_probe(root, 'probe_dpp_novel', novel), downpour_probe(root, 'probe_dpp_longlife', longlife),
            downpour_probe(root, 'probe_dpp_noosc', noosc), downpour_probe(root, 'probe_dpp_noexp', noexp),
            downpour_probe(root, 'probe_dpp_lowrate', lowrate)]


def probe_downpour_flags(root):
    return [downpour_probe(root, f'probe_dpfull_f{flags}', lambda d, f=flags: d.update({'flags': f})) for flags in (0, 4, 1, 2, 5)]


def probe_downpour_bit1(root):
    def f1(sign=None, directions=None):
        def fn(d):
            d['flags'] = 1
            if sign: d['emitter'][0]['sign'] = sign
            if directions: d['emitter'][0]['directions'] = directions
        return fn
    return [downpour_probe(root, 'probe_dpb1_zneg', f1('10 10 -10')), downpour_probe(root, 'probe_dpb1_zpos', f1('10 10 10')),
            downpour_probe(root, 'probe_dpb1_flat', f1(directions='1 1 0'))]


def probe_velshift(root):
    outs = []
    for tag, vel in (('still', None), ('down', '0 -500 0')):
        doc = particle_doc(6, starttime=4, maxcount=100000)
        doc['emitter'] = [{'directions': '1 1 0', 'distancemax': 300, 'distancemin': 0, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 2500}]
        doc['initializer'][0] = {'id': 2, 'max': 1, 'min': 1, 'name': 'lifetimerandom'}
        if vel:
            doc['initializer'].append({'id': 4, 'max': vel, 'min': vel, 'name': 'velocityrandom'})
            doc['operator'] = [{'gravity': '0 0 0', 'id': 7, 'name': 'movement'}]
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/v.json': doc,
                 'scene.json': scene([particle_object(1, tag, '960 700 0', 'particles/v.json')])}
        outs.append(write(root, f'probe_velshift_{tag}', files))
    return outs


def probe_bit1_depth(root):
    outs = []
    for tag, z, flags, bg in (('zpos_bg', 100, 1, True), ('zneg_bg', -100, 1, True), ('zneg_nobg', -100, 1, False), ('zpos_nobg', 100, 1, False),
                              ('zpos_persp_bg', 100, 5, True), ('zneg_persp_bg', -100, 5, True), ('z0_bg', 0, 1, True)):
        doc = particle_doc(100, f'0 0 {z}', flags=flags, renderer={'flags': 1, 'id': 1, 'name': 'sprite', 'orientation': 'screen'})
        objs = []
        if bg:
            objs.append(image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'))
        objs.append(particle_object(2, tag, '960 540 0', 'particles/p.json'))
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc}
        files.update(white_probe_files(objs))
        outs.append(write(root, f'probe_b1d_{tag}', files))
    return outs


def probe_bit1_count(root):
    outs = []
    for tag, flags, life, vel in (('f1_rate', 1, 1, None), ('f0_rate', 0, 1, None), ('f1_long', 1, 20, None), ('f1_long_vel', 1, 20, '0 -100 0')):
        doc = particle_doc(6, starttime=4, maxcount=100000, flags=flags)
        doc['emitter'] = [{'directions': '1 1 0', 'distancemax': 500, 'distancemin': 0, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 2500}]
        doc['initializer'][0] = {'id': 2, 'max': life, 'min': life, 'name': 'lifetimerandom'}
        if vel:
            doc['initializer'].append({'id': 4, 'max': vel, 'min': vel, 'name': 'velocityrandom'})
            doc['operator'] = [{'gravity': '0 0 0', 'id': 7, 'name': 'movement'}]
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/r.json': doc,
                 'scene.json': scene([particle_object(1, tag, '960 540 0', 'particles/r.json')])}
        outs.append(write(root, f'probe_b1c_{tag}', files))
    return outs


def probe_bit1_z(root):
    outs = []
    for z in (300, 600, 1200, -600, -1200, -5000):
        for flags in (1, 0):
            tag = f'z{z}_f{flags}'
            doc = particle_doc(100, f'0 0 {z}', flags=flags, renderer={'flags': 1, 'id': 1, 'name': 'sprite', 'orientation': 'screen'})
            files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc}
            files.update(white_probe_files([image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'),
                                            particle_object(2, tag, '960 540 0', 'particles/p.json')]))
            outs.append(write(root, f'probe_b1z_{tag}', files))
    return outs


def probe_bit1_bisect(root):
    def whitemat(d):
        pass
    def smallradius(d):
        d['emitter'][0].update({'distancemax': 300, 'distancemin': 0})
    def nosign(d):
        d['emitter'][0].pop('sign', None)
        d['emitter'][0]['directions'] = '1 1 0'
    def simple(d):
        d['emitter'][0].update({'rate': 2500, 'instantaneous': 0})
        d['initializer'] = [{'id': 2, 'max': 1, 'min': 1, 'name': 'lifetimerandom'}, {'id': 3, 'max': 100, 'min': 100, 'name': 'sizerandom'},
                            {'id': 4, 'max': 0.7, 'min': 0.7, 'name': 'alpharandom'}]
        d['operator'] = [{'gravity': '0 0 0', 'id': 7, 'name': 'movement'}]
    outs = []
    for tag, fn in (('whitemat', whitemat), ('smallradius', smallradius), ('nosign', nosign), ('simple', simple)):
        for flags in (1, 0):
            def patch(d, fn=fn, flags=flags):
                fn(d)
                d['flags'] = flags
            name = f'probe_b1b_{tag}_f{flags}'
            folder = downpour_probe(root, name, patch)
            if tag == 'whitemat':
                mat_path = f'{folder}/materials/workshop/2251009624/particle/raindownpour_1.json'
                json.dump(PARTICLE_MAT, open(mat_path, 'w'))
                files = {}
                for dirpath, _, names in os.walk(folder):
                    for fname in names:
                        full = os.path.join(dirpath, fname)
                        rel = os.path.relpath(full, folder)
                        if rel not in ('project.json', 'scene.pkg'):
                            files[rel] = open(full, 'rb').read()
                we_pkg.pack(f'{folder}/scene.pkg', files)
            outs.append(folder)
    return outs


def probe_bit1_zvel(root):
    outs = []
    for tag, vel, flags in (('zvel_pos_f1', '0 0 300', 1), ('zvel_neg_f1', '0 0 -300', 1), ('zvel_pos_f0', '0 0 300', 0), ('xvel_f1', '300 0 0', 1)):
        doc = particle_doc(100, flags=flags, renderer={'flags': 1, 'id': 1, 'name': 'sprite', 'orientation': 'screen'}, maxcount=3)
        doc['emitter'][0]['rate'] = 0.7
        doc['initializer'][0] = {'id': 2, 'max': 4, 'min': 4, 'name': 'lifetimerandom'}
        doc['initializer'].append({'id': 4, 'max': vel, 'min': vel, 'name': 'velocityrandom'})
        doc['operator'] = [{'gravity': '0 0 0', 'id': 7, 'name': 'movement'}]
        doc['starttime'] = 0
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc}
        files.update(white_probe_files([image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'),
                                        particle_object(2, tag, '960 540 0', 'particles/p.json')]))
        outs.append(write(root, f'probe_b1v_{tag}', files))
    return outs


def probe_oneshot(root):
    outs = []
    for tag, maxcount, rate, life, flags in (('m3_r5_l1_f1', 3, 5, 1, 1), ('m3_r5_l1_f0', 3, 5, 1, 0), ('m3_r5_l1_f5', 3, 5, 1, 5),
                                              ('m50_r5_l1_f1', 50, 5, 1, 1), ('m3_r5_l100_f1', 3, 5, 100, 1), ('m20_r20_l1_f1', 20, 20, 1, 1)):
        doc = particle_doc(60, flags=flags, maxcount=maxcount, starttime=0)
        doc['emitter'] = [{'distancemax': '600 250 0', 'id': 7, 'name': 'boxrandom', 'origin': '0 0 0', 'rate': rate}]
        doc['initializer'][0] = {'id': 2, 'max': life, 'min': life, 'name': 'lifetimerandom'}
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
                 'scene.json': scene([particle_object(2, tag, '960 540 0', 'particles/p.json')])}
        outs.append(write(root, f'probe_os_{tag}', files))
    return outs


def probe_depth_cull(root):
    outs = []
    for z in (-2000, -3000, -4000, 2000, 3000, 4000, -2400, -2600, 2400, 2600, -2500, 2500):
        doc = particle_doc(100, f'0 0 {z}', renderer={'flags': 1, 'id': 1, 'name': 'sprite', 'orientation': 'screen'})
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc}
        files.update(white_probe_files([image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'),
                                        particle_object(2, f'z{z}', '960 540 0', 'particles/p.json')]))
        outs.append(write(root, f'probe_dc_z{z}', files))
    return outs


def probe_worldspace(root):
    outs = []
    for tag, flags, vel in (('ws3', 1, None), ('ls3', 0, None), ('ws3vel', 1, '200 0 0'), ('ls3vel', 0, '200 0 0')):
        doc = particle_doc(100, '100 0 0', flags=flags, maxcount=1 if vel is None else 6, starttime=0 if vel else 2)
        if vel:
            doc['emitter'][0]['rate'] = 5
            doc['initializer'][0] = {'id': 2, 'max': 1, 'min': 1, 'name': 'lifetimerandom'}
            doc['initializer'].append({'id': 4, 'max': vel, 'min': vel, 'name': 'velocityrandom'})
            doc['operator'] = [{'gravity': '0 0 0', 'id': 7, 'name': 'movement'}]
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
                 'scene.json': scene([particle_object(2, tag, '600 540 0', 'particles/p.json', scale='3 3 1')])}
        outs.append(write(root, f'probe_ws_{tag}', files))
    return outs


def probe_downpour_world(root):
    def novel(d):
        d['flags'] = 1
        d['initializer'] = [i for i in d['initializer'] if i['name'] != 'velocityrandom']
    def flat(d):
        d['flags'] = 1
        d['emitter'][0]['directions'] = '1 1 0'
        for i in d['initializer']:
            if i['name'] == 'velocityrandom':
                i['min'] = '-500 -250 0'; i['max'] = '500 -750 0'
    def longlife(d):
        d['flags'] = 1
        for i in d['initializer']:
            if i['name'] == 'lifetimerandom':
                i['min'] = i['max'] = 5.0
    return [downpour_probe(root, 'probe_dpw_novel', novel), downpour_probe(root, 'probe_dpw_flat', flat), downpour_probe(root, 'probe_dpw_longlife', longlife)]


def probe_world_morph(root):
    dp_files = pkg_files(f'{we_pkg.WORKSHOP}/3597803619/scene.pkg', DOWNPOUR_FILES)
    dp = dp_files[DOWNPOUR_FILES[0]]
    outs = []
    steps = ['material', 'emitter', 'inits', 'rate', 'renderer', 'anim']
    for n in range(1, len(steps) + 1):
        doc = particle_doc(100, '100 0 0', flags=1, maxcount=1, starttime=2)
        files = {'materials/probe_particle.json': PARTICLE_MAT}
        if n >= 1:
            files.update({k: v for k, v in dp_files.items() if k != DOWNPOUR_FILES[0]})
            doc['material'] = DOWNPOUR_FILES[1]
        if n >= 2:
            doc['emitter'] = json.loads(json.dumps(dp['emitter']))
            doc['emitter'][0]['rate'] = 5
            doc['maxcount'] = 20
        if n >= 3:
            doc['initializer'] = json.loads(json.dumps(dp['initializer']))
            doc['operator'] = json.loads(json.dumps(dp['operator']))
        if n >= 4:
            doc['emitter'][0]['rate'] = 25000
            doc['maxcount'] = 25000
        if n >= 5:
            doc['renderer'] = json.loads(json.dumps(dp['renderer']))
        if n >= 6:
            doc['animationmode'] = 'randomframe'
        files['particles/p.json'] = doc
        files.update(white_probe_files([image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'),
                                        particle_object(2, f'morph{n}', '699.0957 622.87366 0', 'particles/p.json', scale='1.74781 1.74781 2.21329')]))
        outs.append(write(root, f'probe_morph_{n}_{steps[n-1]}', files))
    return outs


def probe_world_count(root):
    outs = []
    for tag, flags, rate, maxcount in (('w25k', 1, 25000, 100000), ('l25k', 0, 25000, 100000), ('w25k_pool2k', 1, 25000, 2000), ('w10k', 1, 10000, 100000), ('w5k', 1, 5000, 100000)):
        doc = particle_doc(6, starttime=4, maxcount=maxcount, flags=flags)
        doc['emitter'] = [{'directions': '1 1 0', 'distancemax': 500, 'distancemin': 0, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': rate}]
        doc['initializer'][0] = {'id': 2, 'max': 1, 'min': 1, 'name': 'lifetimerandom'}
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/r.json': doc,
                 'scene.json': scene([particle_object(1, tag, '960 540 0', 'particles/r.json')])}
        outs.append(write(root, f'probe_wc_{tag}', files))
    return outs


def probe_world_emitter(root):
    outs = []
    variants = {
        'sphere_full': {'directions': '1 1 0.3', 'distancemax': 2000, 'distancemin': 1, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 20, 'sign': '10 10 10', 'flags': 0},
        'sphere_nosign': {'directions': '1 1 0.3', 'distancemax': 2000, 'distancemin': 1, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 20},
        'sphere_flat': {'directions': '1 1 0', 'distancemax': 2000, 'distancemin': 1, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 20, 'sign': '10 10 10'},
        'sphere_small': {'directions': '1 1 0.3', 'distancemax': 300, 'distancemin': 1, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 20, 'sign': '10 10 10'},
        'box_wide': {'distancemax': '800 400 0', 'id': 6, 'name': 'boxrandom', 'origin': '0 0 0', 'rate': 20},
    }
    for tag, emitter in variants.items():
        doc = particle_doc(60, flags=1, maxcount=40, starttime=3)
        doc['emitter'] = [emitter]
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc}
        files.update(white_probe_files([image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3'),
                                        particle_object(2, tag, '699.0957 622.87366 0', 'particles/p.json', scale='1.74781 1.74781 2.21329')]))
        outs.append(write(root, f'probe_we_{tag}', files))
    return outs


def probe_world_scale(root):
    outs = []
    for tag, scale in (('s221', '2 2 1'), ('s222', '2 2 2'), ('s112', '1 1 2'), ('s331', '3 3 1'), ('s17521', '1.75 1.75 2.21')):
        doc = particle_doc(40, flags=1, maxcount=6, starttime=3)
        doc['emitter'] = [{'directions': '1 0 0', 'distancemax': 300, 'distancemin': 300, 'id': 6, 'name': 'sphererandom', 'origin': '0 0 0', 'rate': 20, 'sign': '1 0 0'}]
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
                 'scene.json': scene([particle_object(2, tag, '400 540 0', 'particles/p.json', scale=scale)])}
        outs.append(write(root, f'probe_wsc_{tag}', files))
    return outs


def probe_osc_exp(root):
    outs = []
    doc = particle_doc(30, maxcount=60, starttime=6)
    doc['emitter'][0]['rate'] = 30
    doc['operator'] = [{'frequencymax': 1.0, 'frequencymin': 1.0, 'id': 8, 'mask': '100 0 0', 'name': 'oscillateposition', 'phasemax': 6.28, 'phasemin': 0, 'scalemax': 10, 'scalemin': 10}]
    files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
             'scene.json': scene([particle_object(2, 'osc', '960 540 0', 'particles/p.json')])}
    outs.append(write(root, 'probe_osc_mask100', files))
    doc = particle_doc(30, maxcount=60, starttime=6)
    doc['emitter'][0]['rate'] = 30
    doc['operator'] = [{'frequencymax': 1.0, 'frequencymin': 1.0, 'id': 8, 'mask': '1 0 0', 'name': 'oscillateposition', 'phasemax': 6.28, 'phasemin': 0, 'scalemax': 10, 'scalemin': 10}]
    files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
             'scene.json': scene([particle_object(2, 'osc', '960 540 0', 'particles/p.json')])}
    outs.append(write(root, 'probe_osc_mask1', files))
    doc = particle_doc(30, maxcount=40, starttime=6)
    doc['emitter'] = [{'distancemax': '800 400 0', 'id': 7, 'name': 'boxrandom', 'origin': '0 0 0', 'rate': 20}]
    doc['initializer'][1] = {'exponent': 30, 'id': 3, 'max': 200, 'min': 20, 'name': 'sizerandom'}
    files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
             'scene.json': scene([particle_object(2, 'sizeexp', '960 540 0', 'particles/p.json')])}
    outs.append(write(root, 'probe_size_exp30', files))
    doc = json.loads(json.dumps(doc)); doc['initializer'][1]['exponent'] = 0.1
    files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
             'scene.json': scene([particle_object(2, 'sizeexp', '960 540 0', 'particles/p.json')])}
    outs.append(write(root, 'probe_size_exp01', files))
    return outs



def probe_downpour_osc(root):
    def osc(d):
        return next(o for o in d['operator'] if o['name'] == 'oscillateposition')
    variants = {'xy': lambda d: osc(d).update({'mask': '75 75 0'}), 'z': lambda d: osc(d).update({'mask': '0 0 25'}),
                'static': lambda d: osc(d).update({'frequencymin': 0.001, 'frequencymax': 0.001}),
                'small': lambda d: osc(d).update({'scalemin': 1, 'scalemax': 1}),
                'nophase': lambda d: osc(d).update({'phasemin': 0, 'phasemax': 0})}
    return [downpour_probe(root, f'probe_dpo_{name}', patch) for name, patch in variants.items()]



def probe_osc_model(root):
    outs = []
    for name, op in (('probe_osc_init', {'frequencymax': 0.0001, 'frequencymin': 0.0001, 'mask': '1 0 0', 'phasemax': 6.28, 'phasemin': 0, 'scalemax': 1000, 'scalemin': 1000}),
                     ('probe_osc_alias', {'frequencymax': 600, 'frequencymin': 600, 'mask': '1 0 0', 'phasemax': 0, 'phasemin': 0, 'scalemax': 100, 'scalemin': 100})):
        doc = particle_doc(30, maxcount=60, starttime=6)
        doc['emitter'][0]['rate'] = 30
        doc['operator'] = [dict(op, id=8, name='oscillateposition')]
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
                 'scene.json': scene([particle_object(2, 'osc', '960 540 0', 'particles/p.json')])}
        outs.append(write(root, name, files))
    return outs



def probe_osc_track(root):
    doc = particle_doc(30, starttime=2)
    doc['operator'] = [{'frequencymax': 1.0, 'frequencymin': 1.0, 'id': 8, 'mask': '1 0 0', 'name': 'oscillateposition', 'phasemax': 0, 'phasemin': 0, 'scalemax': 300, 'scalemin': 300}]
    files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
             'scene.json': scene([particle_object(2, 'osc', '960 540 0', 'particles/p.json')])}
    return write(root, 'probe_osc_track', files)



def probe_osc_age(root):
    doc = particle_doc(30, starttime=8, maxcount=10)
    doc['emitter'][0]['rate'] = 1
    doc['operator'] = [{'frequencymax': 1.0, 'frequencymin': 1.0, 'id': 8, 'mask': '1 0 0', 'name': 'oscillateposition', 'phasemax': 0, 'phasemin': 0, 'scalemax': 300, 'scalemin': 300}]
    files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
             'scene.json': scene([particle_object(2, 'osc', '960 540 0', 'particles/p.json')])}
    return write(root, 'probe_osc_age', files)



def probe_osc_dt(root):
    outs = []
    for name, frequency in (('probe_osc_dt30', 188.4956), ('probe_osc_dt60', 376.9911), ('probe_osc_dt20', 125.6637)):
        doc = particle_doc(30, starttime=2)
        doc['operator'] = [{'frequencymax': frequency, 'frequencymin': frequency, 'id': 8, 'mask': '1 0 0', 'name': 'oscillateposition', 'phasemax': 0, 'phasemin': 0, 'scalemax': 10, 'scalemin': 10}]
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
                 'scene.json': scene([particle_object(2, 'osc', '960 540 0', 'particles/p.json')])}
        outs.append(write(root, name, files))
    return outs



def probe_count_override(root):
    outs = []
    for name, count in (('probe_ovr_full', None), ('probe_ovr_half', 0.5), ('probe_ovr_fifth', 0.2)):
        doc = particle_doc(40, maxcount=4000, starttime=8)
        doc['emitter'][0]['rate'] = 60
        doc['initializer'][0] = {'id': 2, 'max': 6.0, 'min': 6.0, 'name': 'lifetimerandom'}
        doc['emitter'][0] = {'distancemax': '900 500 0', 'id': 7, 'name': 'boxrandom', 'origin': '0 0 0', 'rate': 60}
        override = None if count is None else {'id': 1002, 'count': count}
        files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc,
                 'scene.json': scene([particle_object(2, 'ovr', '960 540 0', 'particles/p.json', override=override)])}
        outs.append(write(root, name, files))
    return outs



def dark_bg():
    return image_layer(1, 'bg', '960 540 0', '1920 1080', color='0.3 0.3 0.3')


def grey_bg(oid=1):
    return image_layer(oid, 'backdrop', '960 540 0', '1920 1080', color='0.5 0.5 0.5')


def motion_doc(size, life, rate, maxcount, origin='0 0 0', instantaneous=0):
    doc = particle_doc(size, starttime=0, maxcount=maxcount)
    doc['emitter'] = [{'distancemax': '0 0 0', 'id': 7, 'name': 'boxrandom', 'origin': origin, 'rate': rate, 'instantaneous': instantaneous}]
    doc['initializer'] = [{'id': 2, 'max': life, 'min': life, 'name': 'lifetimerandom'}, {'id': 3, 'max': size, 'min': size, 'name': 'sizerandom'}]
    doc['operator'] = [{'drag': 0, 'gravity': '0 0 0', 'id': 8, 'name': 'movement'}]
    return doc


def particle_probe(root, name, doc, extra_objs=()):
    files = {'materials/probe_particle.json': PARTICLE_MAT, 'particles/p.json': doc}
    files.update(white_probe_files([dark_bg(), *extra_objs, particle_object(2, name, '960 540 0', 'particles/p.json')]))
    return write(root, name, files)


def probe_turb(root):
    outs = []
    for tag, kw in (('s01', dict(scale=0.1, offset=0)), ('s05', dict(scale=0.5, offset=0)), ('s1', dict(scale=1, offset=0)),
                    ('s2', dict(scale=2, offset=0)), ('s01_on', dict(scale=0.1, offset=-0.5)), ('s01_op', dict(scale=0.1, offset=0.5)),
                    ('s01_fx', dict(scale=0.1, offset=0, forward='1 0 0')), ('s01_fdown', dict(scale=0.1, offset=0, forward='0 -1 0')),
                    ('s01_rx', dict(scale=0.1, offset=0.5, right='1 0 0'))):
        doc = motion_doc(6, 0.6, 1500, 4000)
        init = {'id': 5, 'name': 'turbulentvelocityrandom', 'speedmin': 400, 'speedmax': 400}
        init.update(kw)
        doc['initializer'].append(init)
        outs.append(particle_probe(root, f'probe_turb_{tag}', doc))
    return outs


def probe_vortex(root):
    outs = []
    base = {'id': 9, 'name': 'vortex', 'controlpoint': 0, 'distanceinner': 500, 'distanceouter': 650, 'speedinner': 5, 'speedouter': 0}
    for tag, op, origin in (('std', dict(base), '200 0 0'),
                            ('keep', dict(base, flags=2, centerforce=5), '200 0 0'),
                            ('ring', dict(base, flags=4, ringradius=300, ringwidth=100, ringpulldistance=200, ringpullforce=5), '450 0 0'),
                            ('axis', dict(base, flags=1), '200 0 0'),
                            ('fast', dict(base, speedinner=60), '200 0 0')):
        doc = motion_doc(8, 60, 0, 400, origin=origin, instantaneous=200)
        doc['operator'].append(op)
        outs.append(particle_probe(root, f'probe_vortex_{tag}', doc, [image_layer(3, 'centre', '960 540 0', '10 10', color='1 0 0')]))
    return outs


def probe_mapseq(root):
    outs = []
    for tag, count, speed, rate, life, cp, origin in (('c8', 8, '0 200 0', 40, 1.0, '0 0 0', '0 0 0'), ('c3', 3, '200 0 0', 3, 0.9, '0 0 0', '0 0 0'),
                                                     ('c3y', 3, '0 200 0', 3, 0.9, '0 0 0', '0 0 0'), ('cp', 8, '0 200 0', 40, 1.0, '300 0 0', '-300 0 0'),
                                                     ('cp2', 8, '0 200 0', 40, 1.0, '300 0 0', '0 0 0'), ('cp3', 8, '0 200 0', 40, 1.0, '300 0 0', '100 -200 0'),
                                                     ('cp4', 8, '0 200 0', 40, 1.0, '0 0 0', '100 -200 0')):
        doc = motion_doc(8, life, rate, 200, origin=origin)
        doc['controlpoint'][0]['offset'] = cp
        doc['initializer'].append({'id': 4, 'name': 'mapsequencearoundcontrolpoint', 'controlpoint': 0, 'count': count, 'speedmin': speed, 'speedmax': speed})
        outs.append(particle_probe(root, f'probe_mapseq_{tag}', doc, [image_layer(3, 'centre', '960 540 0', '10 10', color='1 0 0')]))
    return outs


def probe_align(root):
    objs = [grey_bg()]
    for i, (name, align, scale) in enumerate((('c', 'center', '1 1 1'), ('t', 'top', '1 1 1'), ('b', 'bottom', '1 1 1'), ('l', 'left', '1 1 1'),
                                              ('r', 'right', '1 1 1'), ('ts', 'top', '2 2 1'))):
        x = 200 + i * 300
        objs.append(image_layer(10 + i, name, f'{x} 540 0', '200 100', color='1 0 0', scale=scale, alignment=align))
        objs.append(image_layer(20 + i, name + '-mark', f'{x} 540 0', '6 6', color='0 0 1'))
    return write(root, 'probe_align', white_probe_files(objs))


def probe_ortho(root):
    outs = []
    for tag, auto, objs in (('off', False, [image_layer(1, 'backdrop', '300 200 0', '600 400', color='0.5 0.5 0.5')]),
                            ('a', True, [image_layer(1, 'backdrop', '300 200 0', '600 400', color='0.5 0.5 0.5')]),
                            ('b', True, [image_layer(1, 'backdrop', '300 200 0', '600 400', color='0.5 0.5 0.5'),
                                         image_layer(2, 'far', '900 600 0', '100 100', color='1 0 0')]),
                            ('c', True, [image_layer(1, 'backdrop', '300 200 0', '600 400', color='0.5 0.5 0.5'),
                                         image_layer(2, 'wide', '400 150 0', '900 300', color='1 0 0')]),
                            ('d', True, [image_layer(1, 'backdrop', '300 200 0', '600 400', color='0.5 0.5 0.5'),
                                         image_layer(2, 'small', '300 200 0', '300 100', color='1 0 0')]),
                            ('e', True, [image_layer(1, 'backdrop', '500 300 0', '600 400', color='0.5 0.5 0.5'),
                                         image_layer(2, 'mark', '500 300 0', '40 40', color='1 0 0')])):
        outs.append(write(root, f'probe_ortho_{tag}', white_probe_files(objs, orthogonalprojection={'auto': auto, 'height': 1080, 'width': 1920})))
    return outs


def probe_deps(root):
    outs = []
    for tag, deps in (('ab', [2]), ('self', [1]), ('none', None)):
        a = image_layer(1, 'A', '900 540 0', '400 400', color='1 0 0')
        b = image_layer(2, 'B', '1020 540 0', '400 400', color='0 0 1')
        if deps:
            a['dependencies'] = deps
        outs.append(write(root, f'probe_deps_{tag}', white_probe_files([grey_bg(3), a, b])))
    return outs


def probe_brightness(root):
    objs = [grey_bg(), image_layer(2, 'half', '400 540 0', '200 200', brightness=0.5), image_layer(3, 'one', '800 540 0', '200 200', brightness=1.0),
            image_layer(4, 'double-grey', '1200 540 0', '200 200', color='0.4 0.4 0.4', brightness=2.0),
            image_layer(5, 'double-white', '1600 540 0', '200 200', brightness=2.0)]
    return write(root, 'probe_brightness', white_probe_files(objs))


def probe_shake(root):
    objs = [grey_bg(), image_layer(2, 'cross-h', '960 540 0', '400 6', color='1 0 0'), image_layer(3, 'cross-v', '960 540 0', '6 400', color='1 0 0')]
    outs = []
    for tag, amp, speed, rough in (('a1', 1.0, 1.0, 1.0), ('a3', 3.0, 1.0, 1.0), ('a1s3', 1.0, 3.0, 1.0), ('r4', 1.0, 1.0, 4.0), ('r0', 1.0, 1.0, 0.0)):
        outs.append(write(root, f'probe_shake_{tag}', white_probe_files(objs, camerashake=True, camerashakeamplitude=amp, camerashakespeed=speed, camerashakeroughness=rough)))
    return outs


SCENE_GETTER_SCRIPT = """'use strict';
export function init(value) {
    const layer = thisLayer;
    const fov = thisScene.fov;
    const amount = thisScene.cameraparallaxamount;
    const clear = thisScene.clearcolor;
    const bloom = thisScene.bloomstrength;
    thisLayer.origin = new Vec3(100 + (Number.isFinite(fov) ? fov * 10 : 0), 900, 0);
    thisScene.getLayer('amount').origin = new Vec3(100 + (Number.isFinite(amount) ? amount * 1000 : 0), 700, 0);
    thisScene.getLayer('clear').origin = new Vec3(100 + (clear && Number.isFinite(clear.x) ? clear.x * 1000 : 0), 500, 0);
    thisScene.getLayer('bloom').origin = new Vec3(100 + (Number.isFinite(bloom) ? bloom * 100 : 0), 300, 0);
    return value;
}
"""

SCENE_SETTER_SCRIPT = """'use strict';
export function init(value) {
    thisScene.clearcolor = new Vec3(1, 0, 0);
    return value;
}
"""


def probe_scene_get(root):
    objs = [image_layer(2, 'fov', '100 900 0', '40 40', color='1 0 0'), image_layer(3, 'amount', '100 700 0', '40 40', color='0 1 0'),
            image_layer(4, 'clear', '100 500 0', '40 40', color='0 0 1'), image_layer(5, 'bloom', '100 300 0', '40 40', color='1 1 0')]
    objs[0]['alpha'] = {'script': SCENE_GETTER_SCRIPT, 'value': 1.0}
    for o in objs:
        o['parallaxDepth'] = '0 0'
    return write(root, 'probe_scene_get', white_probe_files(objs, clearcolor='0.3 0 0', cameraparallax=False, cameraparallaxamount=0.5, bloom=False, bloomstrength=2.5))


def probe_scene_set(root):
    objs = [image_layer(2, 'dot', '960 540 0', '100 100', color='0 0 1')]
    objs[0]['alpha'] = {'script': SCENE_SETTER_SCRIPT, 'value': 1.0}
    return write(root, 'probe_scene_set', white_probe_files(objs))



PROBE_VERT = """
uniform mat4 g_ModelViewProjectionMatrix;
attribute vec3 a_Position;
attribute vec2 a_TexCoord;
varying vec2 v_TexCoord;
void main() {
	gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);
	v_TexCoord = a_TexCoord;
}
"""

COMPOSITE_FRAG = """
#include "common_composite.h"
uniform sampler2D g_Texture0; // {"material":"framebuffer","hidden":true}
varying vec2 v_TexCoord;
void main() {
	gl_FragColor = vec4(g_CompositeColor, 1.0);
}
"""

MATRIX_FRAG = """
uniform sampler2D g_Texture0; // {"material":"framebuffer","hidden":true}
uniform MATTYPE MATNAME;
varying vec2 v_TexCoord;
void main() {
	int i = int(floor(v_TexCoord.x * 16.0));
	float e = 0.0;
	ELEMENTS
	float k = v_TexCoord.y < 0.5 ? KFINE : KCOARSE;
	float v = clamp(0.5 + e * k, 0.0, 1.0);
	gl_FragColor = vec4(v, v, v, 1.0);
}
"""

MATRIX_PROBES = {'etp': ('g_EffectTextureProjectionMatrix', 4, 1.0, 0.25), 'etpi': ('g_EffectTextureProjectionMatrixInverse', 4, 0.1, 0.025),
                 'vp': ('g_ViewProjectionMatrix', 4, 200.0, 0.25), 'em': ('g_EffectModelMatrix', 4, 0.002, 0.25),
                 'nm': ('g_NormalModelMatrix', 3, 1.0, 0.25), 'mvp': ('g_ModelViewProjectionMatrix', 4, 200.0, 0.25)}


def matrix_frag(name, rows, kfine, kcoarse):
    elements = []
    for col in range(rows):
        for row in range(rows):
            i = col * 4 + row
            elements.append(('if' if not elements else 'else if') + f' (i == {i}) e = {name}[{col}][{row}];')
    return (MATRIX_FRAG.replace('MATTYPE', f'mat{rows}').replace('MATNAME', name).replace('ELEMENTS', '\n\t'.join(elements))
            .replace('KFINE', f'{kfine:.6f}').replace('KCOARSE', f'{kcoarse:.6f}'))


BIG_MODEL = {'material': 'materials/big.json', 'autosize': False, 'fullscreen': False, 'passthrough': False, 'puppet': None}
BIG_MAT = {'passes': [{'alphawriting': 'default', 'blending': 'translucent', 'combos': {'VERSION': 2}, 'cullmode': 'nocull',
                       'depthtest': 'disabled', 'depthwrite': 'disabled', 'shader': 'genericimage2', 'textures': ['util/perlin_256']}]}


def custom_effect_files(tag, frag):
    return {'models/big.json': BIG_MODEL, 'materials/big.json': BIG_MAT,
            f'effects/{tag}/effect.json': {'name': tag, 'group': 'probe', 'passes': [{'material': f'materials/effects/{tag}.json'}],
                                           'dependencies': [f'materials/effects/{tag}.json', f'shaders/effects/{tag}.frag', f'shaders/effects/{tag}.vert']},
            f'materials/effects/{tag}.json': {'passes': [{'shader': f'effects/{tag}', 'blending': 'normal', 'depthtest': 'disabled', 'depthwrite': 'disabled', 'cullmode': 'nocull'}]},
            f'shaders/effects/{tag}.vert': PROBE_VERT.encode(), f'shaders/effects/{tag}.frag': frag.encode()}


def effect_layer(oid, name, origin, size, tag, color='1 1 1', angles='0 0 0', scale='1 1 1', consts=None):
    o = image_layer(oid, name, origin, size, color=color, angles=angles, scale=scale, model='models/big.json')
    o['effects'] = [{'file': f'effects/{tag}/effect.json', 'id': 100 + oid, 'name': '', 'passes': [{'constantshadervalues': consts or {}, 'id': 200 + oid}], 'visible': True}]
    return o


def probe_composite(root):
    files = custom_effect_files('probe_composite', COMPOSITE_FRAG)
    objs = [grey_bg(), effect_layer(2, 'grey-object', '400 540 0', '300 300', 'probe_composite', color='0.5 0.5 0.5'),
            effect_layer(3, 'white-object', '900 540 0', '300 300', 'probe_composite'),
            effect_layer(4, 'grey-set', '1400 540 0', '300 300', 'probe_composite', color='0.5 0.5 0.5', consts={'compositecolor': '1 0 0'})]
    files.update(white_probe_files(objs))
    return write(root, 'probe_composite', files)


def probe_matrix(root):
    outs = []
    for tag, (name, rows, kfine, kcoarse) in MATRIX_PROBES.items():
        files = custom_effect_files(f'probe_{tag}', matrix_frag(name, rows, kfine, kcoarse))
        objs = [image_layer(1, 'backdrop', '960 540 0', '1920 1080', color='0 0.5 0'),
                effect_layer(2, tag, '700 400 0', '640 320', f'probe_{tag}', angles='0 0 30', scale='1.5 1 1')]
        files.update(white_probe_files(objs))
        outs.append(write(root, f'probe_mat_{tag}', files))
    return outs



BRIGHT_FRAG = """
uniform sampler2D g_Texture0; // {"material":"framebuffer","hidden":true}
uniform float g_Brightness;
varying vec2 v_TexCoord;
void main() {
	float v = clamp(g_Brightness * 0.25, 0.0, 1.0);
	gl_FragColor = vec4(v, v, v, 1.0);
}
"""


def probe_ubright(root):
    files = custom_effect_files('probe_ubright', BRIGHT_FRAG)
    objs = [grey_bg(), effect_layer(2, 'b2', '400 540 0', '300 300', 'probe_ubright'), effect_layer(3, 'b1', '900 540 0', '300 300', 'probe_ubright'),
            effect_layer(4, 'b05', '1400 540 0', '300 300', 'probe_ubright')]
    objs[1]['brightness'] = 2.0
    objs[3]['brightness'] = 0.5
    files.update(white_probe_files(objs))
    return write(root, 'probe_ubright', files)



def probe_brightness_legacy(root):
    objs = [grey_bg(), image_layer(2, 'half', '400 540 0', '200 200', brightness=0.5, model='models/legacy.json'),
            image_layer(3, 'one', '800 540 0', '200 200', brightness=1.0, model='models/legacy.json'),
            image_layer(4, 'double-grey', '1200 540 0', '200 200', color='0.4 0.4 0.4', brightness=2.0, model='models/legacy.json'),
            image_layer(5, 'half-v2', '1600 540 0', '200 200', brightness=0.5)]
    files = white_probe_files(objs)
    files['models/legacy.json'] = {'material': 'materials/legacy.json', 'autosize': False, 'fullscreen': False, 'passthrough': False, 'puppet': None}
    files['materials/legacy.json'] = WHITE_MAT
    return write(root, 'probe_brightness_legacy', files)


PROBES = {name[6:]: fn for name, fn in list(globals().items()) if name.startswith('probe_') and callable(fn)}

if __name__ == '__main__':
    root = sys.argv[1]
    names = sys.argv[2:] or ['all']
    for name in (PROBES if names == ['all'] else names):
        print(PROBES[name](root))
