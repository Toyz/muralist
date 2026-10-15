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

Backends describe what they can do with trait impls, not flags. There are two
surfaces, `Desktop` and `Lock`, and three abilities per surface:

| Trait          | Meaning                                         |
|----------------|-------------------------------------------------|
| `SetOne<S>`    | One image across every monitor                  |
| `SetEach<S>`   | A different image per monitor                   |
| `ReadBack<S>`  | Report what is set now (Peek restores from it)  |

A GNOME backend might look like this:

```rust
pub struct Gnome;

impl Backend for Gnome {
    const ID: &'static str = "gnome";
    const NAME: &'static str = "GNOME";
    // Only compiles if the impls below exist.
    const CAPS: Caps<Self> = Caps { desktop: Ops::one().read(), lock: Some(Ops::one()) };

    fn matches(desktop: &str) -> bool {
        desktop.split(':').any(|d| d.eq_ignore_ascii_case("GNOME"))
    }
    fn connect() -> Result<Self> { Ok(Self) }
    fn monitors(&self) -> Result<Vec<Monitor>> { /* ... */ }
}

impl SetOne<Desktop> for Gnome { /* gsettings set org.gnome.desktop.background ... */ }
impl ReadBack<Desktop> for Gnome { /* ... */ }
impl SetOne<Lock> for Gnome { /* org.gnome.desktop.screensaver ... */ }
```

Then add it to the `backends!` list in `src/backend/mod.rs`, `cfg`-gated if
it is OS-specific. That generates the `Session` enum variant, detection and
dispatch.

Things the compiler checks for you:

- `CAPS` can't claim an ability the backend doesn't implement: `Ops::each()`
  requires `SetEach`, `.read()` requires `ReadBack`, and so on.
- Per-monitor support (`Reach`) comes from whether `SetEach` is implemented,
  so it can't drift from the code.
- The UI only gets a handle to a surface the backend supports, so it can't
  call into one that isn't there. Controls for missing surfaces are hidden.

A surface with only `SetOne` still works with per-monitor assignments: the
primary monitor's image is used everywhere.

`src/backend/plasma/` is the reference implementation, and the `Fake` backend
in the tests at the bottom of `src/backend/mod.rs` is a small example.
GNOME, Sway/wlroots (`swaybg`, `swww`) and Hyprland (`hyprpaper`) would all be
welcome.

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
