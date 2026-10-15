# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-10-15

### Library and History

- Saved wallpapers remember what they are: purity, category, colors and
  tags are kept for every file, looked up automatically (and at once when
  you select one), with cards scanning as they're identified.
- Filter the Library by purity, category and tags, grouped by tag kind, with
  an animated funnel showing how many survive each filter.
- History is a timeline grouped by day, with how long each set stayed up,
  restore and apply on hover, and a month calendar colored by what you wore.

### Galleries

- Edit a gallery after creating it: name, search, purity, categories,
  sorting, shape and how many to keep.
- Right-click a gallery for a quick popout to change its top-level filters,
  refresh, keep offline, rotate or delete it.
- Tag searches show the tag's name and purity instead of `id:123`.

### Inspector, Info and Similar

- The preview no longer jumps while loading: it eases between shapes, shows
  the grid's thumbnail at once, and shows download progress over the
  picture until the original lands.
- Info and Similar open from two tiles and grow out of them: readouts that
  count up, where the wallpaper came from (source, uploader, Wallhaven,
  your machine), and tags that search on click.
- The fit check is now a fit lab: span and fill scored out of 100, the part
  of the picture a span would crop, a model of your desk with each screen's
  slice, and a sharpness gauge per screen.
- Similar ranks lookalikes by palette match, and hovering one puts it side
  by side with the selection, downloading its original.
- Similar works again: Wallhaven now blocks `like:` searches from apps, so
  lookalikes are found by the wallpaper's most telling tag and main color.
  They follow your purity and category filters.

### Elsewhere

- A draggable scrollbar on the grid.
- The lightbox: a shutter rail on the right for where to set it, Set and the
  tools; a cover-flow filmstrip; arrows that preview the next wallpaper;
  download progress over the picture.
- Right-click card: screens drawn as small monitors that preview the
  wallpaper on hover.
- Redesigned side headers.

### Reliability

- Every saved file now carries a format version. Older files are backed up
  as `<name>.v<N>.bak` before being upgraded, and a file written by a newer
  muralist is backed up before it is ever overwritten.
- Saved files are written atomically. A file that fails to read is kept
  aside as `.corrupt-<time>` instead of being replaced by an empty one, and
  a broken config is kept as `config.toml.bad` instead of stopping the app.
- The config file (which holds your API key) is only readable by you.
- Fixed a crash when a `like:` search contained non-ASCII characters.
- Fixed downloads of the same file corrupting each other, half-written
  thumbnails being served from the cache, and a thumbnail worker dying on a
  malformed image.
- Fixed random-order smart galleries repeating results across pages, and
  Plasma losing track of wallpapers whose paths contain spaces or `#`.
- Much less work per frame: the grid, History and Library no longer rebuild
  lists, read files or decode images on every redraw.

## [0.1.0] - 2026-10-14

First release.

### Browsing

- Wallhaven search that starts from your account's own defaults.
- Smart search: `color:`, `ratio:`, `res:`, `sort:`, `cat:` and `purity:`
  filters, suggestions for words that might be filters, and tag, similar and
  uploader searches shown as pills.
- Search by image (drop a picture on the window).
- Grid and justified mosaic layouts, rainbow ordering, purity color-coding
  and optional pixel-mosaic spoilers for NSFW and Sketchy.
- Info and Similar in a modal: fit check across your monitors, palette with
  X-ray, tags, uploader and source.

### Setting wallpapers

- Per-monitor desktop and lock screen on KDE Plasma 6; per-monitor desktop
  and a single lock image on Windows; per-screen desktop on macOS.
- The right side as one flow: wallpaper, where (Desktop, Lock screen or
  Both), screens, apply or discard.
- Span one image across all screens with a framing editor.
- Spin for a random set, or Match the other screens to your selection.
- Right-click a card to lift it into a menu with your screens and actions;
  the original starts downloading the moment you do.
- Hold Space to preview on the real desktop; history of everything set.

### Galleries and rotation

- Hand-picked galleries, smart galleries from saved searches, and imported
  Wallhaven collections, with offline originals.
- Auto-rotate from search, library or a gallery, shuffled to the time of day
  or in order.

### Under the hood

- Parallel range downloads with a band-by-band develop effect, and cursor
  trajectory prefetching.
- Adaptive theme from each wallpaper's palette, with enforced text contrast.
- Compile-time checked desktop backends.
- An LRU cache with a size limit.
