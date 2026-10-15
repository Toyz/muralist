//! Spin: monitors in the Displays panel flicker through results like slot
//! reels and stop one by one. Harmonize uses the same reels but rigs the
//! outcome: wallpapers whose palettes sit closest together in Lab space.

use std::time::{Duration, Instant};

use gpui_kit::*;

use super::{Item, Muralist, Placement, Target, parse_hex};
use std::sync::Arc;

use crate::wallhaven::Wallpaper;

const TICK: Duration = Duration::from_millis(70);
const FIRST_STOP: Duration = Duration::from_millis(900);
const STOP_STAGGER: Duration = Duration::from_millis(550);
const LINGER: Duration = Duration::from_millis(700);

pub(super) struct Reel {
    pub monitor: String,
    pub outcome: Arc<Wallpaper>,
    stop_at: Instant,
    /// Index into the slot's pool while spinning.
    pub showing: usize,
    pub stopped: bool,
}

pub(super) struct Slot {
    pub reels: Vec<Reel>,
    pub pool: Vec<Arc<Wallpaper>>,
    targets: Vec<Target>,
    rng: u64,
    label: &'static str,
}

impl Slot {
    fn next(&mut self) -> u64 {
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
}

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9e37_79b9_7f4a_7c15, |d| d.as_nanos() as u64)
        | 1
}

/// Single-monitor shaped (not ultrawide, not portrait).
pub(super) fn fits_one_screen(w: &Wallpaper) -> bool {
    w.aspect().is_some_and(|r| (1.3..=2.0).contains(&r))
}

/// sRGB -> CIE Lab (D65), so distances track perceived difference.
pub(super) fn lab(c: Hsla) -> [f32; 3] {
    let rgb: Rgba = c.into();
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (lin(rgb.r), lin(rgb.g), lin(rgb.b));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16. / 116. };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116. * fy - 16., 500. * (fx - fy), 200. * (fy - fz)]
}

fn delta_e(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Symmetric chamfer distance between two palettes; dominant colors count double.
pub(super) fn palette_distance(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return f32::MAX;
    }
    let one_way = |x: &[[f32; 3]], y: &[[f32; 3]]| {
        let mut total = 0.;
        let mut weight = 0.;
        for (i, c) in x.iter().enumerate() {
            let w = if i == 0 { 2. } else { 1. };
            let nearest = y.iter().map(|d| delta_e(*c, *d)).fold(f32::MAX, f32::min);
            total += nearest * w;
            weight += w;
        }
        total / weight
    };
    f32::midpoint(one_way(a, b), one_way(b, a))
}

pub(super) fn palette(w: &Wallpaper) -> Vec<[f32; 3]> {
    w.colors.iter().filter_map(|c| parse_hex(c)).map(lab).collect()
}

/// Every reel has stopped and the result has lingered long enough to read.
/// Measured from when the last reel stopped, never from `now`, or the
/// linger would keep moving and the spin would never end.
fn spin_done(reels: &[Reel], now: Instant) -> bool {
    let Some(last_stop) = reels.iter().map(|r| r.stop_at).max() else { return true };
    reels.iter().all(|r| r.stopped) && now >= last_stop + LINGER
}

impl Muralist {
    /// Monitors left to right, primary first when choosing who gets the anchor.
    fn monitor_order(&self) -> Vec<String> {
        let mut order: Vec<_> = self.monitors.iter().collect();
        order.sort_by_key(|m| (!m.primary, m.x));
        order.into_iter().map(|m| m.name.clone()).collect()
    }

    pub(super) fn spin(&mut self, cx: &mut Context<Self>) {
        let mut candidates: Vec<Arc<Wallpaper>> = self.results.iter().filter(|w| fits_one_screen(w)).cloned().collect();
        if candidates.len() < self.monitors.len() {
            candidates = self.results.clone();
        }
        if candidates.is_empty() || self.monitors.is_empty() {
            self.set_status("Search for something to spin through first", true, cx);
            return;
        }
        let mut rng = seed();
        let mut outcome = Vec::new();
        for name in self.monitor_order() {
            if candidates.is_empty() {
                break;
            }
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let pick = candidates.remove((rng % candidates.len() as u64) as usize);
            outcome.push((name, pick));
        }
        self.start_reels(outcome, "Spun", cx);
    }

    pub(super) fn harmonize(&mut self, cx: &mut Context<Self>) {
        let candidates: Vec<Arc<Wallpaper>> =
            self.results.iter().filter(|w| fits_one_screen(w) && !w.colors.is_empty()).cloned().collect();
        let needed = self.monitors.len();
        if candidates.len() < needed || needed == 0 {
            self.set_status("Not enough wallpapers in these results to harmonize", true, cx);
            return;
        }
        // Anchor on the selection when it qualifies, else the crowd favorite.
        let anchor = match &self.selected {
            Some(Item::Remote(w)) if fits_one_screen(w) && !w.colors.is_empty() => w.clone(),
            _ => candidates.iter().max_by_key(|w| w.favorites).cloned().unwrap(),
        };
        let anchor_pal = palette(&anchor);
        let mut ranked: Vec<(f32, Arc<Wallpaper>)> = candidates
            .into_iter()
            .filter(|w| w.id != anchor.id)
            .map(|w| (palette_distance(&anchor_pal, &palette(&w)), w))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));

        let order = self.monitor_order();
        let mut picks = vec![anchor];
        picks.extend(ranked.into_iter().take(needed - 1).map(|(_, w)| w));
        let outcome = order.into_iter().zip(picks).collect();
        self.start_reels(outcome, "Harmonized", cx);
    }

    fn start_reels(&mut self, outcome: Vec<(String, Arc<Wallpaper>)>, label: &'static str, cx: &mut Context<Self>) {
        let now = Instant::now();
        let mut pool: Vec<Arc<Wallpaper>> =
            self.results.iter().filter(|w| self.thumbs.contains_key(&format!("wh-{}", w.id))).cloned().collect();
        if pool.len() < 6 {
            pool = self.results.clone();
        }
        let reels = outcome
            .into_iter()
            .enumerate()
            .map(|(i, (monitor, outcome))| Reel {
                monitor,
                outcome,
                stop_at: now + FIRST_STOP + STOP_STAGGER * i as u32,
                showing: 0,
                stopped: false,
            })
            .collect();
        self.slot = Some(Slot { reels, pool, targets: self.target.targets(), rng: seed(), label });
        cx.notify();

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let finished = this
                    .update(cx, |this, cx| {
                        let Some(slot) = &mut this.slot else {
                            return true;
                        };
                        let now = Instant::now();
                        let pool_len = slot.pool.len().max(1) as u64;
                        for i in 0..slot.reels.len() {
                            let pick = (slot.next() % pool_len) as usize;
                            let reel = &mut slot.reels[i];
                            if !reel.stopped {
                                if now >= reel.stop_at {
                                    reel.stopped = true;
                                } else {
                                    reel.showing = pick;
                                }
                            }
                        }
                        cx.notify();
                        if spin_done(&slot.reels, now) {
                            this.finish_reels(cx);
                            return true;
                        }
                        false
                    })
                    .unwrap_or(true);
                if finished {
                    break;
                }
            }
        })
        .detach();
    }

    fn finish_reels(&mut self, cx: &mut Context<Self>) {
        let Some(slot) = self.slot.take() else {
            return;
        };
        let count = slot.reels.len();
        // The faces stay up while the originals download.
        self.landing = slot.reels.iter().map(|r| (r.monitor.clone(), r.outcome.clone())).collect();
        for reel in slot.reels {
            self.assign_item(Item::Remote(reel.outcome), slot.targets.clone(), Placement::Monitor(reel.monitor), cx);
        }
        self.set_status(format!("{} {count} wallpapers. Press Set to put them up.", slot.label), false, cx);
    }

    /// What a spinning (or freshly stopped) reel shows on `monitor`.
    pub(super) fn reel_face(&self, monitor: &str) -> Option<(Arc<Wallpaper>, bool)> {
        let Some(slot) = self.slot.as_ref() else {
            return self.landing.iter().find(|(m, _)| m == monitor).map(|(_, w)| (w.clone(), true));
        };
        let reel = slot.reels.iter().find(|r| r.monitor == monitor)?;
        if reel.stopped {
            Some((reel.outcome.clone(), true))
        } else {
            slot.pool.get(reel.showing).cloned().map(|w| (w, false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FIRST_STOP, LINGER, Reel, lab, palette_distance, spin_done};
    use crate::wallhaven::{Thumbs, Wallpaper};
    use std::time::Instant;

    fn reel(stop_at: Instant, stopped: bool) -> Reel {
        let outcome = Wallpaper {
            id: "x".into(),
            url: String::new(),
            purity: crate::wallhaven::Purity::Sfw,
            category: crate::wallhaven::Category::General,
            resolution: "1920x1080".into(),
            file_size: 0,
            favorites: 0,
            path: String::new(),
            thumbs: Thumbs { original: String::new(), small: String::new() },
            colors: Vec::new(),
        };
        Reel { monitor: "DP-1".into(), outcome: std::sync::Arc::new(outcome), stop_at, showing: 0, stopped }
    }

    #[test]
    fn spin_finishes_after_the_linger() {
        let start = Instant::now();
        let stop = start + FIRST_STOP;
        // Still spinning.
        assert!(!spin_done(&[reel(stop, false)], start));
        // Stopped, but not lingered yet.
        assert!(!spin_done(&[reel(stop, true)], stop));
        // Stopped long ago: done (this used to hang forever).
        assert!(spin_done(&[reel(stop, true)], stop + LINGER * 3));
    }
    use gpui_kit::hsla;

    #[test]
    fn similar_palettes_are_closer() {
        let blue = [lab(hsla(0.62, 0.7, 0.4, 1.)), lab(hsla(0.6, 0.5, 0.2, 1.))];
        let navy = [lab(hsla(0.63, 0.7, 0.3, 1.)), lab(hsla(0.6, 0.4, 0.15, 1.))];
        let orange = [lab(hsla(0.07, 0.9, 0.55, 1.)), lab(hsla(0.05, 0.6, 0.3, 1.))];
        assert!(palette_distance(&blue, &navy) < palette_distance(&blue, &orange));
    }
}
