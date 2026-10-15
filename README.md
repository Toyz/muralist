<div align="center">

<img src="https://raw.githubusercontent.com/Toyz/muralist/main/docs/logo.svg" width="96" alt="muralist logo">

# muralist

**The most over-engineered wallpaper manager you will ever install.**

Search [Wallhaven](https://wallhaven.cc), then put wallpapers on every monitor,
desktop and lock screen alike. Native, GPU-rendered, built with [GPUI](https://www.gpui.rs/).

[![CI](https://github.com/Toyz/muralist/actions/workflows/ci.yml/badge.svg)](https://github.com/Toyz/muralist/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/muralist.svg)](https://crates.io/crates/muralist)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-8b5cf6.svg)](#license)
![Platforms](https://img.shields.io/badge/platforms-KDE%20Plasma%20%7C%20Windows%20%7C%20macOS-22d3ee.svg)

<br>

<img src="https://raw.githubusercontent.com/Toyz/muralist/main/docs/screenshots/main.webp" alt="muralist: a grid of wallpapers, the selected one tinting the whole interface, and three screens staged on the right">

<sub>The whole interface takes on the selected wallpaper's colors. Pick, place, apply: top to bottom on the right.</sub>

</div>

<br>

## Why

Setting a wallpaper is a solved problem. muralist solves it again, with a
cursor-trajectory predictor that starts downloading the wallpaper you are
about to click, a theme engine that recolors itself around every image while
keeping text above WCAG contrast, and a slot machine. It is, by any reasonable
measure, far too much software for setting a wallpaper. It is also really
nice to use.

<br>

<table>
<tr>
<td width="50%" valign="top">

### The card is the menu

Right-click a wallpaper and the card itself lifts out of the grid and becomes
the menu: your screens along the top, actions on frosted glass in the image's
own colors, a shortcut key on each. The full original starts downloading the
moment you right-click, and sharpens in when it lands.

</td>
<td width="50%">

<img src="https://raw.githubusercontent.com/Toyz/muralist/main/docs/screenshots/context.webp" alt="A wallpaper card lifted out of the grid, its actions on frosted glass">

</td>
</tr>
<tr>
<td width="50%">

<img src="https://raw.githubusercontent.com/Toyz/muralist/main/docs/screenshots/info.webp" alt="Info for a wallpaper: fit check across three displays, palette, tags">

</td>
<td width="50%" valign="top">

### Everything about a wallpaper

Fit check across your real monitors (will it be sharp, how much does a span
crop), its palette with an X-ray that shows where each color lives, uploader,
source and tags, and wallpapers like it. All in one modal, the picture large
beside it.

</td>
</tr>
<tr>
<td width="50%" valign="top">

### Galleries that keep themselves

Hand-pick galleries by dragging cards onto them, save any search as a smart
gallery that refreshes daily (and edit its search any time), or import your
Wallhaven collections. Keep one
offline and its originals download in the background, then let auto-rotate
play through it.

</td>
<td width="50%">

<img src="https://raw.githubusercontent.com/Toyz/muralist/main/docs/screenshots/galleries.webp" alt="The Galleries tab with picture tiles for each gallery">

</td>
</tr>
<tr>
<td width="50%">

<img src="https://raw.githubusercontent.com/Toyz/muralist/main/docs/screenshots/settings.webp" alt="Settings, Auto-rotate section">

</td>
<td width="50%" valign="top">

### Rotation that reads the clock

Fresh wallpapers on a timer, darker at night and brighter by day, from your
current search, your library, or a gallery, shuffled or in order. The lock
screen can follow along.

</td>
</tr>
</table>

<br>

## Features

**Finding wallpapers**

- Search with your Wallhaven account's own defaults, pulled from your settings.
- A smart search box: `color:`, `ratio:`, `res:`, `sort:`, `cat:` and `purity:`
  apply filters directly, plain words stay plain, and words that might be
  filters are offered as suggestions instead of guessed.
- Tag, similar-image and uploader searches show as pills, never raw IDs.
- Drop any picture on the window to search by image.
- A uniform grid, or a justified mosaic that keeps each wallpaper's real shape.
- SFW, Sketchy and NSFW color-coded on every card; NSFW (or Sketchy too) can
  hide behind a pixel mosaic until hovered.

**Putting them up**

- Per-monitor desktop and lock screen, placed independently or together.
- Span one image across every screen, with a framing editor that knows your
  monitors' real sizes and positions.
- Spin for a random set, or Match the other screens to the one you picked.
- Hold Space to preview a wallpaper on your real desktop; let go to revert.
- History of everything you have set, ready to restore.

**Absurd engineering**

- Full-size originals download over parallel HTTP range requests, and the
  preview develops band by band as the chunks land.
- "Precog" reads your cursor's trajectory and prefetches the card you are
  heading for.
- Every palette is k-means'd, and every text color is pushed until it clears
  contrast against whatever the theme just became.
- Desktop backends declare their abilities through traits checked at compile
  time; a backend cannot claim a lock screen it does not implement.
- The cache is an LRU with a size limit, and anything on screen is never
  evicted.

<br>

## Platform support

| Platform | Desktop | Lock screen | Notes |
|---|---|---|---|
| KDE Plasma 6 (Wayland or X11) | Per monitor | Per monitor | |
| Windows 10 and 11 | Per monitor | One image | Windows Spotlight or group policy can override the lock screen |
| macOS | Per monitor | Follows the desktop | Changes the current Space only; macOS has no lock screen API |
| Other Linux desktops | Browse and download only | | Backends welcome |

Windows and macOS support compiles and passes tests in CI on both, but was
built without access to those machines; reports from real use are very
welcome. Adding a desktop is mostly a matter of implementing a few small
traits, see [CONTRIBUTING.md](CONTRIBUTING.md).

On Plasma, the per-monitor lock screen uses a small wallpaper plugin
(`dev.muralist.perscreen`), installed to `~/.local/share/plasma/wallpapers`
the first time you set a lock screen.

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

<details>
<summary><b>Requirements</b></summary>

<br>

- Rust 1.85 or newer (edition 2024).
- Windows and macOS need nothing beyond the Rust toolchain (and Xcode's
  command line tools on macOS).
- On Plasma: `plasmashell`, `kscreen-doctor`, `kwriteconfig6` and
  `kreadconfig6`, which ship with Plasma 6.
- On Linux, GPUI's native dependencies. On Debian and Ubuntu:

  ```sh
  sudo apt install libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
      libx11-xcb-dev libfontconfig-dev libfreetype-dev libasound2-dev pkg-config
  ```

  On Arch these come with `vulkan-icd-loader`, `wayland`, `libxkbcommon-x11`
  and `fontconfig`.

</details>

## Configuration

Settings live in `~/.config/muralist/config.toml` and are all editable from
the settings window. Most people only need an API key:

```toml
api_key = "your-wallhaven-api-key"
download_dir = "/home/you/Pictures/muralist"
```

The key comes from your [Wallhaven account settings](https://wallhaven.cc/settings/account).
Without one, muralist still works, but Wallhaven only returns SFW results and
ignores your account defaults. `WALLHAVEN_API_KEY` in the environment takes
precedence over the file.

<details>
<summary><b>Other files and variables</b></summary>

<br>

- `~/.config/muralist/state.json`: what is on each monitor, plus history.
- `~/.config/muralist/galleries.json`: your galleries.
- `~/.config/muralist/provenance.json`: what each saved wallpaper is (its
  Wallhaven purity, category, colors), so the Library and History keep it.
- `~/.cache/muralist/`: thumbnails, cached originals and span slices, kept
  under a size limit you choose in Settings.
- `MURALIST_BACKEND` forces a backend by ID (`plasma`, `windows`, `macos`).
  Any other value disables wallpaper setting, which is handy when testing the
  UI.

</details>

## Keyboard

| Key | Where | Action |
|---|---|---|
| Space (hold) | Anywhere | Preview on the real desktop |
| Right-click | A card | Lift it into the menu |
| S / P / G | Lifted card | Set, span, save to gallery |
| F / D / Enter | Lifted card | Find similar, save to library, open |
| 1 to 9 | Lifted card | Set on that screen |
| Left / Right, h / l | Lightbox | Previous / next |
| z | Lightbox | Zoom explorer |
| 0 / 1 | Zoom | Fit / actual pixels |
| + / - | Zoom | Zoom in / out |
| m | Lightbox | Desktop mock-up |
| i | Lightbox | Info panel |
| ? | Lightbox | Show all shortcuts |
| Esc | Anywhere | Close what's open |

## Contributing

Bug reports, design ideas and new desktop backends are all welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) for how the code is laid out and how
backends plug in.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

<sub>Wallpapers belong to their creators. muralist is not affiliated with
Wallhaven. Screenshots show SFW results only.</sub>
