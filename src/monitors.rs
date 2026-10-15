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
