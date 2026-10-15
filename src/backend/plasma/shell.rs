//! Apply wallpapers on KDE Plasma (Wayland or X11).
//!
//! Desktop: plasmashell's scripting interface over D-Bus, one containment per
//! screen, matched to outputs by screen position.
//!
//! Lock screen: `kscreenlockerrc`. Stock Plasma shows one image on every
//! monitor, so for per-monitor images we install a tiny wallpaper plugin
//! (`dev.muralist.perscreen`) that picks its image by the QML `Screen.name`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use serde_json::{Map, Value};

use crate::monitors::Monitor;

pub(super) const LOCK_PLUGIN_ID: &str = "dev.muralist.perscreen";

const PLUGIN_FILES: [(&str, &str); 4] = [
    ("metadata.json", include_str!("../../../plasma/dev.muralist.perscreen/metadata.json")),
    ("contents/ui/main.qml", include_str!("../../../plasma/dev.muralist.perscreen/contents/ui/main.qml")),
    ("contents/ui/config.qml", include_str!("../../../plasma/dev.muralist.perscreen/contents/ui/config.qml")),
    ("contents/config/main.xml", include_str!("../../../plasma/dev.muralist.perscreen/contents/config/main.xml")),
];

fn evaluate_script(script: &str) -> Result<String> {
    let conn = zbus::blocking::Connection::session().context("connecting to the session bus")?;
    let reply = conn
        .call_method(
            Some("org.kde.plasmashell"),
            "/PlasmaShell",
            Some("org.kde.PlasmaShell"),
            "evaluateScript",
            &(script,),
        )
        .context("plasmashell evaluateScript failed (is plasmashell running?)")?;
    Ok(reply.body().deserialize::<String>().unwrap_or_default())
}

fn pos_key(x: i32, y: i32) -> String {
    format!("{x},{y}")
}

/// Set each monitor's desktop wallpaper. Monitors missing from `images` are left alone.
pub(super) fn apply_desktop(images: &BTreeMap<String, PathBuf>, monitors: &[Monitor]) -> Result<()> {
    let mut by_pos = Map::new();
    for m in monitors {
        if let Some(path) = images.get(&m.name) {
            by_pos.insert(pos_key(m.x, m.y), Value::String(file_url(path)));
        }
    }
    if by_pos.is_empty() {
        bail!("nothing assigned to any monitor");
    }
    // FillMode 2 = PreserveAspectCrop.
    let script = format!(
        r#"
var images = {map};
var hit = 0;
var ds = desktops();
for (var i = 0; i < ds.length; i++) {{
    var d = ds[i];
    if (d.screen < 0) continue;
    var g = screenGeometry(d.screen);
    var img = images[g.x + "," + g.y];
    if (!img) continue;
    d.wallpaperPlugin = "org.kde.image";
    d.currentConfigGroup = ["Wallpaper", "org.kde.image", "General"];
    d.writeConfig("Image", img);
    d.writeConfig("FillMode", 2);
    hit++;
}}
print(hit);
"#,
        map = Value::Object(by_pos)
    );
    let out = evaluate_script(&script)?;
    if out.trim() == "0" {
        bail!("plasmashell matched no screens; monitor layout may have changed");
    }
    Ok(())
}

/// Read the current desktop wallpaper of each monitor.
pub(super) fn current_desktop(monitors: &[Monitor]) -> Result<BTreeMap<String, PathBuf>> {
    let out = evaluate_script(
        r#"
var ds = desktops();
var r = [];
for (var i = 0; i < ds.length; i++) {
    var d = ds[i];
    if (d.screen < 0 || d.wallpaperPlugin != "org.kde.image") continue;
    var g = screenGeometry(d.screen);
    d.currentConfigGroup = ["Wallpaper", "org.kde.image", "General"];
    r.push(g.x + "," + g.y + "\t" + d.readConfig("Image"));
}
print(r.join("\n"));
"#,
    )?;
    let mut result = BTreeMap::new();
    for line in out.lines() {
        let Some((pos, img)) = line.split_once('\t') else {
            continue;
        };
        let path = PathBuf::from(img.trim().trim_start_matches("file://"));
        if let Some(m) = monitors.iter().find(|m| pos_key(m.x, m.y) == pos) {
            if path.is_file() {
                result.insert(m.name.clone(), path);
            }
        }
    }
    Ok(result)
}

fn plugin_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("plasma/wallpapers").join(LOCK_PLUGIN_ID)
}

/// Install (or refresh) the per-monitor wallpaper plugin under ~/.local/share.
pub(super) fn install_lock_plugin() -> Result<()> {
    let dir = plugin_dir();
    for (rel, contents) in PLUGIN_FILES {
        let dest = dir.join(rel);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::write(&dest, contents).with_context(|| format!("writing {}", dest.display()))?;
    }
    Ok(())
}

fn kwrite(groups: &[&str], key: &str, value: &str) -> Result<()> {
    let mut cmd = Command::new("kwriteconfig6");
    cmd.args(["--file", "kscreenlockerrc"]);
    for g in groups {
        cmd.args(["--group", g]);
    }
    cmd.args(["--key", key, value]);
    let status = cmd.status().context("running kwriteconfig6")?;
    if !status.success() {
        bail!("kwriteconfig6 failed for {key}");
    }
    Ok(())
}

fn set_lock_single_image(path: &Path) -> Result<()> {
    let p = path.to_string_lossy();
    let g = ["Greeter", "Wallpaper", "org.kde.image", "General"];
    kwrite(&g, "Image", &p)?;
    kwrite(&g, "PreviewImage", &p)?;
    Ok(())
}

/// One image on every monitor, using Plasma's stock image plugin.
pub(super) fn apply_lock_single(path: &Path) -> Result<()> {
    set_lock_single_image(path)?;
    kwrite(&["Greeter"], "WallpaperPlugin", "org.kde.image")
}

/// A different image per monitor via the bundled plugin.
pub(super) fn apply_lock_per_monitor(images: &BTreeMap<String, PathBuf>, monitors: &[Monitor]) -> Result<()> {
    let Some(fallback) = monitors.iter().find_map(|m| images.get(&m.name)).or_else(|| images.values().next()) else {
        bail!("nothing assigned to any monitor");
    };

    // Keys: output name (primary), "x,y" position (if Qt reports a different
    // name), "*" (anything else, e.g. a newly plugged monitor).
    let mut map = Map::new();
    for m in monitors {
        if let Some(path) = images.get(&m.name) {
            let v = Value::String(path.to_string_lossy().into_owned());
            map.insert(m.name.clone(), v.clone());
            map.insert(pos_key(m.x, m.y), v);
        }
    }
    map.insert("*".into(), Value::String(fallback.to_string_lossy().into_owned()));

    install_lock_plugin()?;
    kwrite(&["Greeter", "Wallpaper", LOCK_PLUGIN_ID, "General"], "Images", &Value::Object(map).to_string())?;
    // Keep the stock plugin pointed at something sensible in case the user
    // switches back in System Settings.
    set_lock_single_image(fallback)?;
    kwrite(&["Greeter"], "WallpaperPlugin", LOCK_PLUGIN_ID)
}

fn file_url(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy())
}

fn kread(groups: &[&str], key: &str) -> Option<String> {
    let mut cmd = Command::new("kreadconfig6");
    cmd.args(["--file", "kscreenlockerrc"]);
    for g in groups {
        cmd.args(["--group", g]);
    }
    cmd.args(["--key", key]);
    let out = cmd.output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !s.is_empty()).then_some(s)
}

/// Read the current lock screen image of each monitor.
pub(super) fn current_lock(monitors: &[Monitor]) -> Result<BTreeMap<String, PathBuf>> {
    let plugin = kread(&["Greeter"], "WallpaperPlugin").unwrap_or_else(|| "org.kde.image".into());
    let mut result = BTreeMap::new();
    if plugin == LOCK_PLUGIN_ID {
        let json = kread(&["Greeter", "Wallpaper", LOCK_PLUGIN_ID, "General"], "Images").unwrap_or_default();
        let map: BTreeMap<String, String> = serde_json::from_str(&json).unwrap_or_default();
        for m in monitors {
            if let Some(p) = map.get(&m.name).or_else(|| map.get("*")) {
                result.insert(m.name.clone(), PathBuf::from(p));
            }
        }
    } else if plugin == "org.kde.image" {
        if let Some(img) = kread(&["Greeter", "Wallpaper", "org.kde.image", "General"], "Image") {
            let path = PathBuf::from(img.trim_start_matches("file://"));
            for m in monitors {
                result.insert(m.name.clone(), path.clone());
            }
        }
    }
    result.retain(|_, p| p.is_file());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-applies the current desktop wallpapers (a visual no-op) to exercise
    /// the D-Bus path: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn reapply_current_desktop() {
        let monitors = super::super::kscreen::detect().unwrap();
        let current = current_desktop(&monitors).unwrap();
        assert!(!current.is_empty(), "no org.kde.image wallpapers found");
        apply_desktop(&current, &monitors).unwrap();
        assert_eq!(current_desktop(&monitors).unwrap(), current);
    }
}
