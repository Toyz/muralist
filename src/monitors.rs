//! A display, as every backend reports it.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// Connector name, e.g. "DP-1".
    pub name: String,
    /// Position in the logical desktop coordinate space.
    pub x: i32,
    pub y: i32,
    /// Size in logical pixels (after scale and rotation).
    pub width: u32,
    pub height: u32,
    /// Size in physical pixels (after rotation); what a wallpaper should be rendered at.
    pub px_width: u32,
    pub px_height: u32,
    /// The desktop's primary display.
    pub primary: bool,
}

/// The rectangle all monitors cover: `(min_x, min_y, max_x, max_y)` in
/// logical pixels, never empty (a lone placeholder pixel when there are no
/// monitors), so it's always safe to scale by.
pub fn extent(monitors: &[Monitor]) -> (i32, i32, i32, i32) {
    let right = |m: &Monitor| m.x.saturating_add(i32::try_from(m.width).unwrap_or(i32::MAX));
    let bottom = |m: &Monitor| m.y.saturating_add(i32::try_from(m.height).unwrap_or(i32::MAX));
    let min_x = monitors.iter().map(|m| m.x).min().unwrap_or(0);
    let min_y = monitors.iter().map(|m| m.y).min().unwrap_or(0);
    let max_x = monitors.iter().map(right).max().unwrap_or(1).max(min_x + 1);
    let max_y = monitors.iter().map(bottom).max().unwrap_or(1).max(min_y + 1);
    (min_x, min_y, max_x, max_y)
}
