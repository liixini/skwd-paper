# Third-party code

Mouse coordinate mapping and parallax response in `crates/paper-scene/src/mouse.rs`, `crates/paper-vk/src/app/scene/mouse.rs`, and `crates/paper-vk/src/wayland/pointer.rs` are adapted from [linux-wallpaperengine](https://github.com/Almamu/linux-wallpaperengine), by Almamu and contributors, revision `b016d7d1fdcf4e5fd2f9c9fa420a8aaa07fee02d`.

Upstream files: `src/WallpaperEngine/Input/Drivers/WaylandMouseInput.cpp`, `src/WallpaperEngine/Render/Drivers/WaylandOpenGLDriver.cpp`, `src/WallpaperEngine/Render/Wallpapers/CScene.cpp`, and `src/WallpaperEngine/Render/Objects/CImage.cpp`.

The upstream code is licensed under GNU GPL version 3. The license text is provided in [LICENSE](LICENSE). The Rust adaptation uses Wayland events, retains the last pointer position on focus loss, and schedules redraws only while input or parallax changes. It does not include the upstream Hyprland cursor polling fallback.
