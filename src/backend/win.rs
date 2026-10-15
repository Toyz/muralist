//! Windows 10 and 11: the desktop through the shell's `IDesktopWallpaper` COM
//! interface (per monitor), the lock screen through WinRT's `LockScreen`
//! (one image).
//!
//! Windows has no shared logical coordinate space when monitors use
//! different scales, so monitors are reported in physical pixels throughout.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use windows::Storage::StorageFile;
use windows::System::UserProfile::LockScreen;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromRect,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{DWPOS_FILL, DesktopWallpaper, IDesktopWallpaper};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::{Backend, Caps, Desktop, Images, Lock, Ops, ReadBack, SetEach, SetOne, Shown};
use crate::monitors::Monitor;

pub struct Windows;

impl Backend for Windows {
    const ID: &'static str = "windows";
    const NAME: &'static str = "Windows";
    const CAPS: Caps<Self> = Caps { desktop: Ops::each().read(), lock: Some(Ops::one()) };

    fn matches(_desktop: &str) -> bool {
        true
    }

    fn connect() -> Result<Self> {
        Ok(Self)
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        let _com = Com::init();
        Ok(outputs(&shell()?)?.into_iter().map(|(_, m)| m).collect())
    }
}

impl SetOne<Desktop> for Windows {
    fn set_one(&self, image: &Path, _monitors: &[Monitor]) -> Result<Shown> {
        let _com = Com::init();
        let shell = shell()?;
        fill(&shell)?;
        // A null monitor ID means every monitor.
        unsafe { shell.SetWallpaper(PCWSTR::null(), &wide_path(image)?) }.context("setting the wallpaper")?;
        Ok(None)
    }
}

impl SetEach<Desktop> for Windows {
    fn set_each(&self, images: &Images, _monitors: &[Monitor]) -> Result<Shown> {
        let _com = Com::init();
        let shell = shell()?;
        fill(&shell)?;
        for (id, monitor) in outputs(&shell)? {
            if let Some(path) = images.get(&monitor.name) {
                unsafe { shell.SetWallpaper(&id, &wide_path(path)?) }
                    .with_context(|| format!("setting the wallpaper on {}", monitor.name))?;
            }
        }
        Ok(None)
    }
}

impl ReadBack<Desktop> for Windows {
    fn current(&self, _monitors: &[Monitor]) -> Result<Images> {
        let _com = Com::init();
        let shell = shell()?;
        let mut images = Images::new();
        for (id, monitor) in outputs(&shell)? {
            let path = take(unsafe { shell.GetWallpaper(&id) }?);
            if !path.is_empty() {
                images.insert(monitor.name, PathBuf::from(path.to_os_string()));
            }
        }
        Ok(images)
    }
}

impl SetOne<Lock> for Windows {
    fn set_one(&self, image: &Path, _monitors: &[Monitor]) -> Result<Shown> {
        let _com = Com::init();
        let file = StorageFile::GetFileFromPathAsync(&wide_path(image)?)?.join()?;
        LockScreen::SetImageFileAsync(&file)?
            .join()
            .context("setting the lock screen (Windows Spotlight or a group policy may be managing it)")?;
        Ok(None)
    }
}

/// COM on the calling thread for as long as this lives. Backend calls come
/// from worker threads, so each call sets it up for itself.
struct Com(bool);

impl Com {
    fn init() -> Self {
        // Fails only when the thread already uses another apartment model,
        // which serves just as well; then there is nothing to undo.
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn shell() -> Result<IDesktopWallpaper> {
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }.context("opening the shell's wallpaper interface")
}

/// Images arrive cut to each screen, so cover rather than letterbox.
fn fill(shell: &IDesktopWallpaper) -> Result<()> {
    unsafe { shell.SetPosition(DWPOS_FILL) }.context("setting the wallpaper position")
}

/// Active monitors, left to right, each with the shell's ID for it.
fn outputs(shell: &IDesktopWallpaper) -> Result<Vec<(HSTRING, Monitor)>> {
    let count = unsafe { shell.GetMonitorDevicePathCount() }?;
    let mut out = Vec::new();
    for i in 0..count {
        let id = take(unsafe { shell.GetMonitorDevicePathAt(i) }?);
        // Disconnected monitors keep an ID but have no rectangle.
        let Ok(rect) = (unsafe { shell.GetMonitorRECT(&id) }) else { continue };
        let (w, h) = ((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32);
        if w == 0 || h == 0 {
            continue;
        }

        let handle = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        let known = unsafe { GetMonitorInfoW(handle, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) }.as_bool();
        // "\\.\DISPLAY2" -> "DISPLAY2", the number Windows shows in Settings.
        let name = if known {
            let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
            String::from_utf16_lossy(&info.szDevice[..len]).trim_start_matches(r"\\.\").to_owned()
        } else {
            format!("DISPLAY{}", i + 1)
        };

        out.push((
            id,
            Monitor {
                name,
                x: rect.left,
                y: rect.top,
                width: w,
                height: h,
                px_width: w,
                px_height: h,
                primary: known && info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            },
        ));
    }
    out.sort_by_key(|(_, m)| (m.x, m.y));
    Ok(out)
}

/// Copies a string the shell allocated, then frees it.
fn take(s: PWSTR) -> HSTRING {
    let copy = unsafe { s.to_hstring() };
    unsafe { CoTaskMemFree(Some(s.0 as *const _)) };
    copy
}

fn wide_path(path: &Path) -> Result<HSTRING> {
    let path = std::path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
    Ok(HSTRING::from(path.as_os_str()))
}
