# Skwd-paper

Repository for the wayland wallpaper daemon Skwd-paper, which can be used completely standalone from the Skwd-wall suite. If you are on KDE Plasma you also need [skwd-paper-plasma](https://github.com/liixini/skwd-paper-plasma)

## Install

### Arch Linux, CachyOS, EndeavourOS, Manjaro

| Action | Command |
| --- | --- |
| Install Paper | `yay -S skwd-paper-bin` |
| KDE Plasma plugin | `yay -S skwd-paper-plasma` |

### COPR: Fedora, Nobara

| Action | Command |
| --- | --- |
| Enable COPR | `sudo dnf copr enable piixini/skwd-wall-v2` |
| Install Paper | `sudo dnf install skwd-paper` |
| KDE Plasma plugin | `sudo dnf install skwd-paper-plasma` |

## Apply

| Action | Command |
| --- | --- |
| List monitors | `skwd-paper-v2 outputs` |
| Image on one monitor | `skwd-paper-v2 apply DP-1 image.jpg` |
| Image on two monitors | `skwd-paper-v2 apply DP-1,DP-2 image.jpg` |
| Fit the image inside the monitor | `skwd-paper-v2 apply DP-1 image.jpg --fill-mode fit` |
| Span an image across all monitors | `skwd-paper-v2 apply '*' image.jpg --fill-mode span` |
| Loop a muted video on all monitors | `skwd-paper-v2 apply '*' video.mp4 --mute true` |
| Animated GIF | `skwd-paper-v2 apply DP-1 animation.gif` |
| Wallpaper Engine scene | `skwd-paper-v2 apply DP-1 /path/to/scene --kind we` |
| Fade to an image over 800 ms | `skwd-paper-v2 apply DP-1 next.jpg --transition --duration-ms 800` |
| Sand transition to an image | `skwd-paper-v2 apply DP-1 next.jpg --effect sand-mobius --duration-ms 1200` |
| Fade to a video over 800 ms | `skwd-paper-v2 apply DP-1 next.mp4 --transition --duration-ms 800` |
| Apply an image and stop wallpapers on other monitors | `skwd-paper-v2 apply DP-1 image.jpg --replace-all` |
| Apply wallpapers from a JSON manifest | `skwd-paper-v2 apply --manifest @wallpapers.json` |

## Transitions

| Transition | Option |
| --- | --- |
| Fade (default) | `--effect fade` |
| Random | `--effect random` |
| Pixelate | `--effect pixelate` |
| Iris | `--effect iris` |
| Glitch | `--effect glitch` |
| Voronoi shatter | `--effect voronoi-shatter` |
| Heat melt | `--effect heat-melt` |
| Plasma flow | `--effect plasma-flow` |
| Ink splash | `--effect ink-splash` |
| Smoke | `--effect smoke` |
| Chromatic bloom | `--effect chromatic-bloom` |
| Inkwell drop | `--effect inkwell-drop` |
| Pixelfade wave | `--effect pixelfade-wave` |
| Soft warp fade | `--effect soft-warp-fade` |
| Zoom blur pull | `--effect zoom-blur-pull` |
| Mosaic tumble | `--effect mosaic-tumble` |
| Crosswarp | `--effect crosswarp` |
| Morph | `--effect morph` |
| Circle crop | `--effect circle-crop` |
| Colour distance | `--effect colour-distance` |
| Crossfade | `--effect crossfade` |
| Directional | `--effect directional` |
| Directional scaled | `--effect directional-scaled` |
| Glitch displace | `--effect glitch-displace` |
| Polka dots curtain | `--effect polka-dots-curtain` |
| Puzzle right | `--effect puzzle-right` |
| Crosshatch | `--effect crosshatch` |
| Directional wipe | `--effect directional-wipe` |
| Fade colour | `--effect fadecolor` |
| Parametric glitch | `--effect parametric-glitch` |
| Perlin | `--effect perlin` |
| Random squares | `--effect randomsquares` |
| Sand bloom | `--effect sand-bloom` |
| Sand maelstrom | `--effect sand-maelstrom` |
| Sand murmuration | `--effect sand-murmuration` |
| Sand donut | `--effect sand-donut` |
| Sand globe | `--effect sand-globe` |
| Sand helix | `--effect sand-helix` |
| Sand galaxy | `--effect sand-galaxy` |
| Sand Möbius | `--effect sand-mobius` |
| Sand tornado | `--effect sand-tornado` |

## Transition settings

| Setting | Option |
| --- | --- |
| Fade from the current wallpaper | `--transition` |
| Choose a transition | `--effect sand-mobius` |
| Choose the starting image or video | `--transition-from previous.jpg` |
| Duration: 50-10000 ms; default 600 ms | `--duration-ms 800` |

## Fill modes

| Mode | Option | Placement |
| --- | --- | --- |
| Fill (default) | `--fill-mode fill` | Keep proportions; crop to cover the monitor |
| Fit | `--fill-mode fit` | Keep proportions; show the whole image with borders where needed |
| Stretch | `--fill-mode stretch` | Fill the monitor without keeping proportions |
| Center | `--fill-mode center` | Center at original size; crop if larger than the monitor |
| Tile | `--fill-mode tile` | Repeat at original size |
| Span | `--fill-mode span` | One image across all monitors; use `'*'` or `ALL` |

## Media types and video engines

| Type or engine | Option | Files and limits |
| --- | --- | --- |
| Automatic (default) | Omit `--kind` | Detect images, animated GIFs, videos and Wallpaper Engine project directories |
| Static image | `--kind static` | Images; display GIFs as a still image |
| Video | `--kind video` | Local videos and animated GIFs |
| Wallpaper Engine | `--kind we` | Supported scene or video projects |
| Default video engine | `--engine default` | Normal video playback, audio and transitions |
| Tinier video engine | `--engine tinier --frame-rate 30000/1001` | AV1 IVF; explicit monitor names, background layer, fill mode only; no audio or transitions |

## SceneScript

Wallpaper Engine scenes load a QuickJS runtime only when they contain scripts that need it. Scenes without scripts allocate no JavaScript heap. Exact matches for the clock and date scripts already supported by Paper keep their native update path.

The current support covers property `init` and `update` functions, saved script properties, vectors, timers, audio buffers, and pointer callbacks. Scripts can change text, layer position, scale, rotation, colour, opacity, visibility, and effect constants. Text textures refresh when their content changes.

Compatibility is partial. Dynamic layer creation, timeline control, media callbacks, local storage, sound control, and scripted particle settings are not implemented. Some helper modules and layer APIs are also incomplete.

Each scripted scene has a 32 MiB JavaScript heap limit. Update callbacks share a 4 ms execution budget per frame. Initialization, cursor callbacks, and transferring changed properties have separate limits. Failed updates are disabled and logged; a runtime-wide failure stops scripting while the renderer keeps its last accepted state. Scripted scenes can retain hidden layers and effect passes so that scripts can change them later, which can increase RAM and GPU memory use.

## Layers

| Layer | Option | Placement |
| --- | --- | --- |
| Background (default) | `--layer background` | Behind desktop windows; required for static images and Tinier |
| Bottom | `--layer bottom` | Above the background layer, below normal windows; video and scenes |
| Top | `--layer top` | Above normal windows; video and scenes |
| Overlay | `--layer overlay` | Above top-layer surfaces; video and scenes |

The direct `skwd-wall-vk` compatibility command defaults to `bottom`. Elevated video and scene surfaces pass pointer input through to the desktop.

## Other apply options

| Setting | Option |
| --- | --- |
| Mute or unmute; default muted | `--mute true` / `--mute false` |
| Volume: 0-100; default 80 | `--volume 30` |
| Pause after 60 seconds without input; requires idle-notify | `--idle-seconds 60 --replace-all` |
| Disable inactivity pausing | `--idle-seconds 0 --replace-all` |
| Scene properties; names depend on the project | `--properties '{"property_name":true}'` |
| Replace all wallpapers, stopping monitors not included | `--replace-all` |
| JSON manifest from a file; maximum 1 MiB | `--manifest @wallpapers.json` |
| JSON manifest from stdin; maximum 1 MiB | `--manifest -` |
| JSON manifest inline; maximum 1 MiB | `--manifest '{"assignments":[{"outputs":["DP-1"],"source":{"kind":"static","path":"image.jpg"}}]}'` |


A manifest can set `policy.surface` to `{"namespace":"skwd-paper-backdrop","blur":12,"dim":20}` for a separate overview surface. Blur and dimming range from 0 to 100 and work with images, default-engine videos, and native Wallpaper Engine scenes. Surface policies keep video players resident when paused so playback resumes from the same position. They do not support transitions or the tinier engine. Set `replace_all` when changing the policy.

Use a separate `SKWD_PAPER_V2_SOCKET` for an independently controlled backdrop. Apply, pause, resume, and stop then affect only that instance.

## Control

| Action | Command |
| --- | --- |
| Pause all wallpapers | `skwd-paper-v2 pause` |
| Resume all wallpapers | `skwd-paper-v2 resume` |
| Unmute a monitor and set volume to 30% | `skwd-paper-v2 audio DP-1 --mute false --volume 30` |
| Mute all wallpapers | `skwd-paper-v2 audio --mute true` |
| Pause after 60 seconds without input (requires idle-notify) | `skwd-paper-v2 apply DP-1 video.mp4 --idle-seconds 60 --replace-all` |
| Show current wallpapers | `skwd-paper-v2 status` |
| Show supported features | `skwd-paper-v2 capabilities` |
| Show installed commands | `skwd-paper-v2 --help` |
| Stop wallpapers on two monitors | `skwd-paper-v2 stop DP-1 DP-2` |
| Stop all wallpapers and exit | `skwd-paper-v2 stop` |

## Moving from awww or mpvpaper

| awww or mpvpaper | Skwd-paper |
| --- | --- |
| `awww img -o DP-1 image.jpg` | `skwd-paper-v2 apply DP-1 image.jpg` |
| `awww img -o DP-1,DP-2 image.jpg` | `skwd-paper-v2 apply DP-1,DP-2 image.jpg` |
| `awww img image.jpg` | `skwd-paper-v2 apply '*' image.jpg` |
| `awww img -o DP-1 animation.gif` | `skwd-paper-v2 apply DP-1 animation.gif` |
| `mpvpaper -o "no-audio loop" DP-1 video.mp4` | `skwd-paper-v2 apply DP-1 video.mp4 --mute true` |
| `mpvpaper --help-output` | `skwd-paper-v2 outputs` |
