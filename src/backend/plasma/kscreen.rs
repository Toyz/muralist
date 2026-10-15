//! Monitor layout from `kscreen-doctor -j`.

use std::process::Command;

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

use crate::monitors::Monitor;

#[derive(Deserialize)]
struct Doc {
    outputs: Vec<Output>,
}

#[derive(Deserialize)]
struct Output {
    name: String,
    enabled: bool,
    #[serde(default)]
    connected: Option<bool>,
    pos: Pos,
    size: Size,
    #[serde(default = "one")]
    scale: f64,
    /// 1 = none, 2 = left, 4 = inverted, 8 = right
    #[serde(default = "one_u")]
    rotation: u32,
    #[serde(default)]
    priority: u32,
}

#[derive(Deserialize)]
struct Pos {
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

fn one() -> f64 {
    1.0
}

fn one_u() -> u32 {
    1
}

pub(super) fn detect() -> Result<Vec<Monitor>> {
    let out = Command::new("kscreen-doctor")
        .arg("-j")
        .output()
        .context("running kscreen-doctor (is this a KDE Plasma session?)")?;
    if !out.status.success() {
        bail!("kscreen-doctor failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    parse(&out.stdout)
}

pub(super) fn parse(json: &[u8]) -> Result<Vec<Monitor>> {
    let doc: Doc = serde_json::from_slice(json).context("parsing kscreen-doctor output")?;
    let mut monitors: Vec<Monitor> = doc
        .outputs
        .into_iter()
        .filter(|o| o.enabled && o.connected.unwrap_or(true))
        .map(|o| {
            let (pw, ph) =
                if matches!(o.rotation, 2 | 8) { (o.size.height, o.size.width) } else { (o.size.width, o.size.height) };
            let scale = if o.scale > 0.0 { o.scale } else { 1.0 };
            Monitor {
                name: o.name,
                x: o.pos.x,
                y: o.pos.y,
                width: (f64::from(pw) / scale).round() as u32,
                height: (f64::from(ph) / scale).round() as u32,
                px_width: pw,
                px_height: ph,
                primary: o.priority == 1,
            }
        })
        .collect();
    monitors.sort_by_key(|m| (m.x, m.y));
    Ok(monitors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_sorts_left_to_right() {
        let json = br#"{"outputs":[
            {"name":"DP-2","enabled":true,"connected":true,"pos":{"x":2560,"y":0},"size":{"width":2560,"height":1440},"scale":1,"rotation":1,"priority":1},
            {"name":"HDMI-1","enabled":false,"connected":true,"pos":{"x":0,"y":0},"size":{"width":1920,"height":1080},"scale":1,"rotation":1},
            {"name":"DP-1","enabled":true,"connected":true,"pos":{"x":0,"y":0},"size":{"width":3840,"height":2160},"scale":1.5,"rotation":8}
        ]}"#;
        let m = parse(json).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].name, "DP-1");
        assert_eq!((m[0].px_width, m[0].px_height), (2160, 3840));
        assert_eq!((m[0].width, m[0].height), (1440, 2560));
        assert_eq!(m[1].name, "DP-2");
        assert!(m[1].primary && !m[0].primary);
    }
}
