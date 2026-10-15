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
    reply.body().deserialize::<String>().context("decoding plasmashell's reply")
}

fn pos_key(x: i32, y: i32) -> String {
    format!("{x},{y}")
}

/// Desktop wallpaper plugins that take a picture under the same `Image` / `FillMode`
/// keys as Plasma's stock image plugin. A screen already running one of these keeps
/// it and is fed the image in that plugin's own config group; any other plugin is
/// replaced by the first entry. `org.helba.hudwall` is a shader wallpaper that
/// renders the image through live effects.
const IMAGE_PLUGINS: [&str; 2] = ["org.kde.image", "org.helba.hudwall"];

/// JS that picks the image plugin for desktop `d`, as variable `p`.
fn pick_plugin_js() -> String {
    format!(
        "var plugins = {list};\n    var p = plugins.indexOf(d.wallpaperPlugin) >= 0 ? d.wallpaperPlugin : plugins[0];",
        list = Value::from(IMAGE_PLUGINS.to_vec())
    )
}

/// The plasmashell script that sets each screen's image; `by_pos` maps "x,y" to a file URL.
fn set_desktop_script(by_pos: Map<String, Value>) -> String {
    // FillMode 2 = PreserveAspectCrop. Only switch plugins when we must: reassigning
    // the same one reloads the wallpaper and restarts a shader plugin's animation.
    format!(
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
    {pick}
    if (d.wallpaperPlugin != p) d.wallpaperPlugin = p;
    d.currentConfigGroup = ["Wallpaper", p, "General"];
    d.writeConfig("Image", img);
    d.writeConfig("FillMode", 2);
    hit++;
}}
print(hit);
"#,
        map = Value::Object(by_pos),
        pick = pick_plugin_js()
    )
}

/// The plasmashell script that prints "x,y<TAB>image URL" for every screen on an image plugin.
fn read_desktop_script() -> String {
    format!(
        r#"
var ds = desktops();
var r = [];
for (var i = 0; i < ds.length; i++) {{
    var d = ds[i];
    if (d.screen < 0) continue;
    {pick}
    if (d.wallpaperPlugin != p) continue;
    var g = screenGeometry(d.screen);
    d.currentConfigGroup = ["Wallpaper", p, "General"];
    r.push(g.x + "," + g.y + "\t" + d.readConfig("Image"));
}}
print(r.join("\n"));
"#,
        pick = pick_plugin_js()
    )
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
    let out = evaluate_script(&set_desktop_script(by_pos))?;
    if out.trim().parse::<u32>().unwrap_or(0) == 0 {
        bail!("plasmashell matched no screens; monitor layout may have changed");
    }
    Ok(())
}

/// Read the current desktop wallpaper of each monitor.
pub(super) fn current_desktop(monitors: &[Monitor]) -> Result<BTreeMap<String, PathBuf>> {
    let out = evaluate_script(&read_desktop_script())?;
    let mut result = BTreeMap::new();
    for line in out.lines() {
        let Some((pos, img)) = line.split_once('\t') else {
            continue;
        };
        let path = url_path(img.trim());
        if let Some(m) = monitors.iter().find(|m| pos_key(m.x, m.y) == pos)
            && path.is_file()
        {
            result.insert(m.name.clone(), path);
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

/// A percent-encoded `file://` URL: names with spaces, `#` or `%` survive.
fn file_url(path: &Path) -> String {
    url::Url::from_file_path(path).map_or_else(|()| format!("file://{}", path.to_string_lossy()), String::from)
}

/// The path in a `file://` URL Plasma hands back, decoded.
fn url_path(url: &str) -> PathBuf {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .unwrap_or_else(|| PathBuf::from(url.trim_start_matches("file://")))
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
    } else if plugin == "org.kde.image"
        && let Some(img) = kread(&["Greeter", "Wallpaper", "org.kde.image", "General"], "Image")
    {
        let path = url_path(&img);
        for m in monitors {
            result.insert(m.name.clone(), path.clone());
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

#[cfg(test)]
mod script_tests {
    use super::*;

    fn set_script() -> String {
        let mut by_pos = Map::new();
        by_pos.insert("0,0".into(), Value::String("file:///a.png".into()));
        set_desktop_script(by_pos)
    }

    #[test]
    fn set_keeps_image_capable_plugins_and_writes_their_group() {
        let s = set_script();
        assert!(s.contains(r#"var plugins = ["org.kde.image","org.helba.hudwall"];"#));
        assert!(s.contains("if (d.wallpaperPlugin != p) d.wallpaperPlugin = p;"));
        assert!(s.contains(r#"d.currentConfigGroup = ["Wallpaper", p, "General"];"#));
        // The old unconditional switch to the stock plugin must be gone.
        assert!(!s.contains(r#"d.wallpaperPlugin = "org.kde.image""#));
    }

    #[test]
    fn read_follows_the_active_image_plugin() {
        let s = read_desktop_script();
        assert!(s.contains("if (d.wallpaperPlugin != p) continue;"));
        assert!(s.contains(r#"d.currentConfigGroup = ["Wallpaper", p, "General"];"#));
        assert!(!s.contains(r#""org.kde.image", "General""#));
    }

    #[test]
    fn stock_plugin_is_the_fallback() {
        assert_eq!(IMAGE_PLUGINS[0], "org.kde.image");
    }
}

#[cfg(test)]
mod url_tests {
    use super::{file_url, url_path};
    use std::path::Path;

    #[test]
    fn odd_file_names_survive_the_round_trip() {
        let path = Path::new("/home/me/Pictures/my wall #2 100%.jpg");
        let url = file_url(path);
        assert!(url.starts_with("file:///") && !url.contains(' '));
        assert_eq!(url_path(&url), path);
    }
}
