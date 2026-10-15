# Contributing

Thanks for taking a look. Bug reports, desktop backends and design ideas are
all welcome.

## Getting started

```sh
git clone https://github.com/Toyz/muralist
cd muralist
cargo run
```

Dev builds optimize dependencies (GPUI is slow without it) but keep the crate
itself at `opt-level = 1`, so rebuilds stay quick.

To keep your real settings out of the way while testing, point the app at a
scratch config:

```sh
XDG_CONFIG_HOME=/tmp/muralist-dev XDG_CACHE_HOME=/tmp/muralist-dev-cache cargo run
```

## Before opening a pull request

```sh
cargo fmt
cargo clippy --all-targets
cargo test
```

Some tests are `#[ignore]`d because they hit the network or a live Plasma
session. Run them explicitly when you touch those areas:

```sh
WALLHAVEN_API_KEY=... cargo test -- --ignored
```

Never commit an API key. Tests read it from the environment.

## Layout

| Path                    | What lives there                                  |
|-------------------------|---------------------------------------------------|
| `src/wallhaven.rs`      | API client, parallel range downloads              |
| `src/backend/`          | The `Backend` trait and per-desktop backends      |
| `src/span.rs`           | Slicing one image across monitors                 |
| `src/thumbs.rs`         | Thumbnail and image-processing worker pool        |
| `src/config.rs`         | Config and state files                            |
| `src/ui.rs`, `src/ui/`  | The GPUI interface                                |
| `plasma/`               | The Plasma lock-screen wallpaper plugin (QML)     |

## Adding a desktop backend

Each desktop implements `Backend` in `src/backend/mod.rs`:

- `capabilities()` says what the desktop can do: per-monitor desktop
  wallpapers, a lock screen, per-monitor lock screens, and whether the
  current wallpaper can be read back. The UI hides controls a backend can't
  support, so be honest here.
- `monitors()` returns connected outputs with their position, size and scale.
  Spanning depends on these being accurate.
- `set_desktop` and `set_lock` receive one image path per monitor.

Then add a match arm to `detect()`, usually keyed on `XDG_CURRENT_DESKTOP`.
`src/backend/plasma/` is the reference implementation. GNOME, Sway/wlroots
(via `swaybg` or `swww`) and Hyprland (`hyprpaper`) would all be welcome.

## Style

- Match the code around you: naming, comment density, module structure.
- Comments explain why, not what.
- UI work: controls go through `kit::Btn` and `kit::surface` so the look stays
  consistent. Colors come from the live palette (`pal()`), never hard-coded,
  and text colors must keep passing the contrast checks in `ui/adaptive.rs`.
- Keep commits focused, with messages that say what changed and why.

## License

By contributing, you agree that your contributions are dual licensed under
MIT and Apache-2.0, as described in the [README](README.md#license).
