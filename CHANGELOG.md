# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
