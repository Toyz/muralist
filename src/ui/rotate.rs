//! Auto-rotate: every N minutes, a fresh color-matched set goes up, darker at
//! night and brighter by day. Runs while muralist is open.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use chrono::Timelike as _;
use gpui_kit::*;

use super::slot::{fits_one_screen, lab, palette, palette_distance};
use super::{Muralist, adaptive};
use crate::backend::Session;
use crate::config::{LockMode, RotateOrder, RotateSource, cache_dir};
use crate::gallery::{Entry, Gallery};
use crate::monitors::Monitor;
use crate::span;
use crate::wallhaven::{Client, SearchParams, Sorting, Wallpaper};

const CHECK_EVERY: Duration = Duration::from_secs(20);
/// Chance of spanning one ultrawide instead of a matched set, when one fits.
const SPAN_CHANCE: u64 = 30;

/// Lightness band (0..1) that suits the current local hour.
fn wanted_brightness() -> (f32, f32) {
    match chrono::Local::now().hour() {
        21..=23 | 0..=5 => (0.0, 0.32),
        6..=8 | 18..=20 => (0.15, 0.6),
        _ => (0.32, 1.0),
    }
}

fn brightness(colors: &[[f32; 3]]) -> f32 {
    if colors.is_empty() {
        return 0.5;
    }
    // Dominant color counts double.
    let total: f32 = colors.iter().enumerate().map(|(i, c)| c[0] * if i == 0 { 2. } else { 1. }).sum();
    total / (colors.len() + 1) as f32 / 100.
}

fn rng(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Everything the background rotation needs, captured up front.
struct Job {
    backend: Arc<Session>,
    client: Client,
    params: SearchParams,
    monitors: Vec<Monitor>,
    download_dir: PathBuf,
    library: Vec<PathBuf>,
    source: RotateSource,
    /// The gallery behind `RotateSource::Gallery`, captured up front.
    gallery: Option<Gallery>,
    order: RotateOrder,
    cursor: usize,
    match_time: bool,
    lock: Option<LockMode>,
}

impl Job {
    /// Puts up a new set. Also returns where in-order playback is up to.
    fn run(self) -> Result<(BTreeMap<String, PathBuf>, Option<usize>)> {
        let mut seed =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64) | 1;
        let (lo, hi) = if self.match_time { wanted_brightness() } else { (0., 1.) };
        let (map, cursor) = match self.source {
            RotateSource::Search => (self.from_search(lo, hi, &mut seed)?, None),
            RotateSource::Library => (self.from_library(lo, hi, &mut seed)?, None),
            RotateSource::Gallery(_) => {
                let gallery = self.gallery.as_ref().context("that gallery no longer exists")?;
                self.from_gallery(gallery, lo, hi, &mut seed)?
            }
        };
        self.backend.desktop().context("no desktop backend")?.set(&map, &self.monitors)?;
        if let (Some(mode), Some(lock)) = (self.lock, self.backend.lock()) {
            lock.apply(&map, &self.monitors, mode)?;
        }
        Ok((map, cursor))
    }

    fn from_gallery(
        &self,
        gallery: &Gallery,
        lo: f32,
        hi: f32,
        seed: &mut u64,
    ) -> Result<(BTreeMap<String, PathBuf>, Option<usize>)> {
        let items = &gallery.items;
        if items.is_empty() {
            bail!("the gallery \"{}\" is empty", gallery.name);
        }
        let n = self.monitors.len();
        let (picks, cursor): (Vec<&Entry>, Option<usize>) = match self.order {
            RotateOrder::InOrder => {
                let start = self.cursor % items.len();
                ((0..n).map(|i| &items[(start + i) % items.len()]).collect(), Some((start + n) % items.len()))
            }
            RotateOrder::Shuffle => {
                // Wallhaven ships each image's palette; local files are measured.
                let scored: Vec<(&Entry, f32)> = items
                    .iter()
                    .map(|e| {
                        let b = match e {
                            Entry::Wallhaven(w) => brightness(&palette(w)),
                            Entry::Local(p) => {
                                brightness(&adaptive::image_palette(p).into_iter().map(lab).collect::<Vec<_>>())
                            }
                        };
                        (e, b)
                    })
                    .collect();
                let mut pool: Vec<&Entry> =
                    scored.iter().filter(|(_, b)| (lo..=hi).contains(b)).map(|(e, _)| *e).collect();
                if pool.is_empty() {
                    pool = items.iter().collect();
                }
                let mut picks = Vec::new();
                for _ in 0..n {
                    let ix = (rng(seed) % pool.len() as u64) as usize;
                    picks.push(if pool.len() > 1 { pool.remove(ix) } else { pool[0] });
                }
                (picks, None)
            }
        };
        let mut map = BTreeMap::new();
        for (m, entry) in self.monitors.iter().zip(picks) {
            map.insert(m.name.clone(), self.original(entry)?);
        }
        Ok((map, cursor))
    }

    /// The entry's full-size original, downloading it if needed.
    fn original(&self, entry: &Entry) -> Result<PathBuf> {
        let path = entry.original(&self.download_dir);
        match entry {
            Entry::Wallhaven(w) if !path.is_file() => {
                let path = self.client.download_parallel(&w.path, &path, &Default::default())?;
                if let Err(e) = crate::provenance::remember(w) {
                    eprintln!("recording provenance: {e:#}");
                }
                Ok(path)
            }
            Entry::Local(p) if !p.is_file() => bail!("{} is missing", p.display()),
            _ => Ok(path),
        }
    }

    fn from_search(&self, lo: f32, hi: f32, seed: &mut u64) -> Result<BTreeMap<String, PathBuf>> {
        let resp = self.client.search(&self.params)?;
        let mut pool: Vec<Wallpaper> = resp.data;
        let in_band: Vec<Wallpaper> =
            pool.iter().filter(|w| (lo..=hi).contains(&brightness(&palette(w)))).cloned().collect();
        if in_band.len() >= self.monitors.len() {
            pool = in_band;
        }
        if pool.is_empty() {
            bail!("no results to rotate through");
        }
        let download = |w: &Wallpaper| -> Result<PathBuf> {
            let path =
                self.client.download_parallel(&w.path, &self.download_dir.join(w.file_name()), &Default::default())?;
            if let Err(e) = crate::provenance::remember(w) {
                eprintln!("recording provenance: {e:#}");
            }
            Ok(path)
        };

        // Sometimes: one ultrawide across everything.
        let ultrawide: Vec<&Wallpaper> = pool
            .iter()
            .filter(|w| {
                w.resolution
                    .split_once('x')
                    .and_then(|(a, b)| Some(a.parse::<f32>().ok()? / b.parse::<f32>().ok()?))
                    .is_some_and(|r| r >= 2.4)
            })
            .collect();
        if self.monitors.len() > 1 && !ultrawide.is_empty() && rng(seed) % 100 < SPAN_CHANCE {
            let pick = ultrawide[(rng(seed) % ultrawide.len() as u64) as usize];
            let path = download(pick)?;
            return span::slice(&path, &self.monitors, &cache_dir().join("span"));
        }

        // Otherwise a random anchor plus its closest palettes.
        let singles: Vec<Wallpaper> = pool.iter().filter(|w| fits_one_screen(w)).cloned().collect();
        let pool = if singles.len() >= self.monitors.len() { singles } else { pool };
        let anchor = pool[(rng(seed) % pool.len() as u64) as usize].clone();
        let anchor_pal = palette(&anchor);
        let mut ranked: Vec<(f32, Wallpaper)> = pool
            .into_iter()
            .filter(|w| w.id != anchor.id)
            .map(|w| (palette_distance(&anchor_pal, &palette(&w)), w))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut picks = vec![anchor];
        picks.extend(ranked.into_iter().map(|(_, w)| w));

        let mut map = BTreeMap::new();
        for (m, w) in self.monitors.iter().zip(picks.iter().cycle()) {
            map.insert(m.name.clone(), download(w)?);
        }
        Ok(map)
    }

    fn from_library(&self, lo: f32, hi: f32, seed: &mut u64) -> Result<BTreeMap<String, PathBuf>> {
        if self.library.is_empty() {
            bail!("library is empty");
        }
        let scored: Vec<(PathBuf, f32)> = self
            .library
            .iter()
            .map(|p| {
                let colors: Vec<[f32; 3]> = adaptive::image_palette(p).into_iter().map(lab).collect();
                (p.clone(), brightness(&colors))
            })
            .collect();
        let mut pool: Vec<PathBuf> =
            scored.iter().filter(|(_, b)| (lo..=hi).contains(b)).map(|(p, _)| p.clone()).collect();
        if pool.is_empty() {
            pool = scored.into_iter().map(|(p, _)| p).collect();
        }
        let mut map = BTreeMap::new();
        for m in &self.monitors {
            let ix = (rng(seed) % pool.len() as u64) as usize;
            let pick = if pool.len() > 1 { pool.remove(ix) } else { pool[0].clone() };
            map.insert(m.name.clone(), pick);
        }
        Ok(map)
    }
}

impl Muralist {
    /// Starts the once-per-`CHECK_EVERY` scheduler. Call once.
    pub(super) fn start_rotation_clock(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_EVERY).await;
                let alive = this
                    .update(cx, |this, cx| {
                        let minutes = this.cfg.rotate_minutes;
                        let due = minutes > 0
                            && !this.rotating
                            && this.last_rotation.elapsed() >= Duration::from_secs(u64::from(minutes) * 60);
                        if due {
                            this.rotate_now(cx);
                        } else if minutes > 0 {
                            cx.notify(); // keep the countdown fresh
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn rotate_now(&mut self, cx: &mut Context<Self>) {
        self.rotate_from(self.cfg.rotate_source, cx);
    }

    /// Puts up a new set from `source`, whatever auto-rotate is set to.
    pub(super) fn rotate_from(&mut self, source: RotateSource, cx: &mut Context<Self>) {
        if self.rotating || self.monitors.is_empty() {
            return;
        }
        self.rotating = true;
        self.last_rotation = Instant::now();
        let job = Job {
            backend: self.backend.clone(),
            client: self.client.clone(),
            params: SearchParams {
                query: self.query.read(cx).value().to_string(),
                categories: self.categories,
                purity: self.purity,
                sorting: Sorting::Random,
                top_range: self.top_range.clone(),
                ratios: None,
                atleast: self.atleast.clone(),
                resolutions: self.resolutions.clone(),
                color: None,
                page: 1,
                seed: None,
            },
            monitors: self.monitors.clone(),
            download_dir: self.cfg.download_dir.clone(),
            library: self.library.clone(),
            source,
            gallery: match source {
                RotateSource::Gallery(id) => self.galleries.get(id).cloned(),
                _ => None,
            },
            order: self.cfg.rotate_order,
            cursor: self.cfg.rotate_cursor,
            match_time: self.cfg.rotate_match_time,
            lock: self.cfg.rotate_lock.then_some(self.cfg.lock_mode),
        };
        let with_lock = job.lock.is_some();
        self.set_status("Rotating wallpapers...", false, cx);
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { job.run() }).await;
            this.update(cx, |this, cx| {
                this.rotating = false;
                this.last_rotation = Instant::now();
                match result {
                    Ok((map, cursor)) => {
                        if let Some(cursor) = cursor {
                            this.cfg.rotate_cursor = cursor;
                            if let Err(e) = this.cfg.save() {
                                eprintln!("saving config: {e:#}");
                            }
                        }
                        this.applied.record("desktop", &map);
                        this.applied.desktop = map.clone();
                        this.desktop = map.clone();
                        if with_lock {
                            this.applied.record("lock", &map);
                            this.applied.lock = map.clone();
                            this.lock = map;
                        }
                        if let Err(e) = this.applied.save() {
                            eprintln!("saving state: {e:#}");
                        }
                        this.refresh_library();
                        let next = this.cfg.rotate_minutes;
                        this.set_status(
                            if next > 0 { format!("Rotated  ·  next in {next} min") } else { "Rotated".into() },
                            false,
                            cx,
                        );
                    }
                    Err(e) => this.set_status(format!("Rotation failed: {e:#}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// "next in 12 min", for the UI.
    pub(super) fn rotation_countdown(&self) -> Option<String> {
        let minutes = self.cfg.rotate_minutes;
        if minutes == 0 {
            return None;
        }
        if self.rotating {
            return Some("rotating now".into());
        }
        let left = (u64::from(minutes) * 60).saturating_sub(self.last_rotation.elapsed().as_secs());
        Some(if left >= 60 { format!("next in {} min", left.div_ceil(60)) } else { "next in under a minute".into() })
    }
}
