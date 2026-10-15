# muralist

A wallpaper manager for multi-monitor Linux desktops. It searches
[Wallhaven](https://wallhaven.cc), downloads at full speed, and puts images on
every monitor, for both the desktop and the lock screen. Built with
[GPUI](https://www.gpui.rs/).

It is, by any reasonable measure, far too much software for setting a
wallpaper.

## Features

**Browsing**

- Search Wallhaven with your account's own defaults (sorting, purity, ratios,
  minimum resolution) pulled from your Wallhaven settings.
- Smart search box: `color:`, `ratio:`, `res:`, `sort:`, `cat:` and `purity:`
  apply filters directly. Plain words stay plain search terms. Words that might
  be filters show up as suggestions you can accept.
- Tag, similar-image and uploader searches appear as pills, not raw IDs.
- Search by image: drop a picture on the window to find similar wallpapers.
- Two layouts: a uniform grid, or a justified mosaic that keeps each
  wallpaper's real shape.
- SFW, Sketchy and NSFW are color-coded on every card. NSFW (or Sketchy too)
  can hide behind a pixel mosaic until hovered.

**Loading**

- Thumbnails stream in with a placeholder built from the image's own palette.
- Full-size downloads use parallel HTTP range requests, and the preview
  develops band by band as chunks arrive.
- Cursor-trajectory prefetching ("precog") starts the download for the card
  you are about to click.

**Setting wallpapers**

- Per-monitor desktop and lock screen, each placed independently.
- Span one image across all monitors, with a framing editor for zoom and pan
  that accounts for real monitor sizes and positions.
- Slot machine: spin for a fresh, color-matched set across all screens, or
  harmonize the other monitors to the one you picked.
- Auto-rotate on a timer, from search results or your downloaded library,
  optionally matched to the time of day.
- History of everything you have set, ready to restore.
- Hold Space to preview a wallpaper on your real desktop, and let go to revert.

**Interface**

- The whole UI takes on the selected wallpaper's colors, with text contrast
  enforced so it stays readable on any image.
- Lightbox with a zoom explorer, minimap, loupe, filmstrip and a desktop
  mock-up showing the image on your monitor layout.

## Platform support

| Desktop     | Desktop wallpaper | Lock screen   | Per monitor |
|-------------|-------------------|---------------|-------------|
| KDE Plasma 6 (Wayland or X11) | yes | yes | yes |
| Others      | browse and download only | | |

Each desktop is a backend that declares what it can do through trait impls,
checked at compile time, so adding one is mostly a matter of implementing a
few small traits. See [CONTRIBUTING.md](CONTRIBUTING.md).

On Plasma, the per-monitor lock screen uses a small wallpaper plugin
(`dev.muralist.perscreen`), installed to `~/.local/share/plasma/wallpapers`
the first time you set a lock screen.

## Requirements

- Rust 1.85 or newer (edition 2024).
- On Plasma: `plasmashell`, `kscreen-doctor`, `kwriteconfig6` and
  `kreadconfig6`, which ship with Plasma 6.
- GPUI's native dependencies. On Debian and Ubuntu:

  ```sh
  sudo apt install libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
      libx11-xcb-dev libfontconfig-dev libfreetype-dev libasound2-dev pkg-config
  ```

  On Arch these come with `vulkan-icd-loader`, `wayland`, `libxkbcommon-x11`
  and `fontconfig`.

## Install

```sh
cargo install muralist
```

Or from source:

```sh
git clone https://github.com/Toyz/muralist
cd muralist
cargo run --release
```

## Configuration

Settings live in `~/.config/muralist/config.toml` and are editable from the
settings dialog. Most people only need an API key:

```toml
api_key = "your-wallhaven-api-key"
download_dir = "/home/you/Pictures/muralist"
```

The key comes from your [Wallhaven account settings](https://wallhaven.cc/settings/account).
Without one, muralist still works but Wallhaven only returns SFW results and
ignores your account defaults. You can also set `WALLHAVEN_API_KEY` in the
environment, which takes precedence over the file.

Other files:

- `~/.config/muralist/state.json`: what is on each monitor, plus history.
- `~/.cache/muralist/`: thumbnails and sliced span images. Safe to clear from
  the settings dialog.

`MURALIST_BACKEND` forces a backend by ID (`plasma`). Any other value
disables wallpaper setting, which is useful when testing the UI.

## Keyboard

| Key              | Where    | Action                         |
|------------------|----------|--------------------------------|
| Space (hold)     | Anywhere | Preview on the real desktop    |
| Left / Right, h / l | Lightbox | Previous / next             |
| z                | Lightbox | Zoom explorer                  |
| 0 / 1            | Zoom     | Fit / actual pixels            |
| + / -            | Zoom     | Zoom in / out                  |
| m                | Lightbox | Desktop mock-up                |
| i                | Lightbox | Info panel                     |
| ?                | Lightbox | Show all shortcuts             |
| Esc              | Lightbox | Close                          |

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

Wallpapers belong to their creators. muralist is not affiliated with
Wallhaven.
