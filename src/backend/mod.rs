//! Desktop backends: everything that talks to a particular desktop
//! environment or OS lives here, behind a [`Session`].
//!
//! What a backend can do is expressed in its trait impls rather than in
//! flags. There are two surfaces, [`Desktop`] and [`Lock`], and per surface
//! three abilities:
//!
//! - [`SetOne`]: put one image on every monitor.
//! - [`SetEach`]: a different image per monitor. That is the [`Reach`].
//! - [`ReadBack`]: report what is set now (Peek restores from this).
//!
//! A backend lists its abilities in [`Backend::CAPS`] using [`Ops`], whose
//! builders only compile when the matching trait is implemented. So a backend
//! can't claim a lock screen it can't set, and per-monitor support can't
//! drift from the code that provides it.
//!
//! The active backend is picked once at startup and kept in a [`Session`],
//! an enum over every compiled-in backend: dispatch is a `match`, not a
//! vtable. The UI reaches a surface through [`Session::desktop`] and
//! [`Session::lock`], which hand out a [`Wall`] only when the backend
//! supports it, so there is no unsupported path to forget to check.
//!
//! Adding a desktop:
//! 1. `backend/<name>/mod.rs` with a type implementing [`Backend`] plus the
//!    surface traits it supports. [`SetOne<Desktop>`] is required.
//! 2. Put any OS-specific crates under `[target.'cfg(...)'.dependencies]`.
//! 3. Add a line to the `backends!` list below, `cfg`-gated if needed.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};

use crate::config::LockMode;
use crate::monitors::Monitor;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "linux")]
mod plasma;
#[cfg(windows)]
mod win;

/// Image per output name.
pub type Images = BTreeMap<String, PathBuf>;

/// How many different images a surface can show at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// One image, shown on every monitor.
    Shared,
    /// A different image on each monitor.
    PerMonitor,
}

/// Somewhere wallpapers go: [`Desktop`] or [`Lock`].
pub trait Surface: Sized + 'static {
    /// This surface's entry in a backend's capability table.
    fn ops<B>(caps: &Caps<B>) -> Option<Ops<B, Self>>;
}

/// The desktop background.
pub enum Desktop {}

/// The lock screen.
pub enum Lock {}

impl Surface for Desktop {
    fn ops<B>(caps: &Caps<B>) -> Option<Ops<B, Self>> {
        Some(caps.desktop)
    }
}

impl Surface for Lock {
    fn ops<B>(caps: &Caps<B>) -> Option<Ops<B, Self>> {
        caps.lock
    }
}

/// What each monitor of a surface shows once a set has gone through, when
/// the backend learns it in the same go (Plasma's script reads every screen
/// back as it sets them); `None` when it can't tell.
pub type Shown = Option<Images>;

/// Put one image on every monitor of surface `S`.
pub trait SetOne<S: Surface> {
    fn set_one(&self, image: &Path, monitors: &[Monitor]) -> Result<Shown>;
}

/// A different image on each monitor of surface `S`.
pub trait SetEach<S: Surface>: SetOne<S> {
    /// Monitors missing from `images` are left alone.
    fn set_each(&self, images: &Images, monitors: &[Monitor]) -> Result<Shown>;
}

/// Report what each monitor of surface `S` shows now.
pub trait ReadBack<S: Surface> {
    fn current(&self, monitors: &[Monitor]) -> Result<Images>;
}

type SetOneFn<B> = fn(&B, &Path, &[Monitor]) -> Result<Shown>;
type SetEachFn<B> = fn(&B, &Images, &[Monitor]) -> Result<Shown>;
type ReadFn<B> = fn(&B, &[Monitor]) -> Result<Images>;

/// What backend `B` can do on surface `S`. Built at compile time; each
/// builder demands the trait that backs it.
pub struct Ops<B, S> {
    one: SetOneFn<B>,
    each: Option<SetEachFn<B>>,
    read: Option<ReadFn<B>>,
    surface: PhantomData<fn() -> S>,
}

impl<B, S> Clone for Ops<B, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<B, S> Copy for Ops<B, S> {}

impl<B, S: Surface> Ops<B, S> {
    /// One image across every monitor.
    pub const fn one() -> Self
    where
        B: SetOne<S>,
    {
        Self { one: <B as SetOne<S>>::set_one, each: None, read: None, surface: PhantomData }
    }

    /// A different image per monitor.
    pub const fn each() -> Self
    where
        B: SetEach<S>,
    {
        Self { each: Some(<B as SetEach<S>>::set_each), ..Self::one() }
    }

    /// Can also report what is set now.
    pub const fn read(self) -> Self
    where
        B: ReadBack<S>,
    {
        Self { read: Some(<B as ReadBack<S>>::current), ..self }
    }

    pub const fn reach(&self) -> Reach {
        if self.each.is_some() { Reach::PerMonitor } else { Reach::Shared }
    }
}

/// A backend's abilities, one entry per surface. The desktop is mandatory.
pub struct Caps<B> {
    pub desktop: Ops<B, Desktop>,
    pub lock: Option<Ops<B, Lock>>,
}

pub trait Backend: Send + Sync + Sized + 'static {
    /// Matched against `MURALIST_BACKEND` to force this backend.
    const ID: &'static str;
    /// Human name, e.g. "KDE Plasma".
    const NAME: &'static str;
    const CAPS: Caps<Self>;

    /// Whether this backend drives the session `desktop`, a
    /// colon-separated `XDG_CURRENT_DESKTOP` value.
    fn matches(desktop: &str) -> bool;

    fn connect() -> Result<Self>;

    /// Enabled displays, sorted left to right.
    fn monitors(&self) -> Result<Vec<Monitor>>;
}

/// An operation over whichever backend is active, with its concrete type.
trait Visit {
    type Out;
    fn backend<B: Backend>(self, b: &B) -> Self::Out;
    fn unsupported(self) -> Self::Out;
}

/// Declares the compiled-in backends and generates [`Session`] over them.
/// Detection tries them in order.
macro_rules! backends {
    ($( $(#[$cfg:meta])* $variant:ident($ty:ty), )*) => {
        /// The backend for this session, chosen once at startup.
        pub enum Session {
            $( $(#[$cfg])* $variant($ty), )*
            /// A desktop with no backend yet: browsing works, setting doesn't.
            Unsupported(String),
        }

        impl Session {
            /// Pick the backend for this session. `MURALIST_BACKEND` forces one.
            pub fn detect() -> Self {
                let forced = std::env::var("MURALIST_BACKEND").ok().map(|b| b.to_ascii_lowercase());
                let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
                $(
                    $(#[$cfg])*
                    {
                        let wanted = match forced.as_deref() {
                            Some(id) => id == <$ty as Backend>::ID,
                            None => <$ty as Backend>::matches(&desktop),
                        };
                        if wanted {
                            match <$ty as Backend>::connect() {
                                Ok(b) => return Self::$variant(b),
                                Err(e) => eprintln!("muralist: {}: {e:#}", <$ty as Backend>::NAME),
                            }
                        }
                    }
                )*
                let _ = forced;
                Self::Unsupported(if desktop.is_empty() { "this desktop".to_owned() } else { desktop })
            }

            fn visit<V: Visit>(&self, v: V) -> V::Out {
                match self {
                    $( $(#[$cfg])* Self::$variant(b) => v.backend(b), )*
                    Self::Unsupported(_) => v.unsupported(),
                }
            }
        }
    };
}

backends! {
    #[cfg(target_os = "linux")]
    Plasma(plasma::Plasma),
    #[cfg(windows)]
    Windows(win::Windows),
    #[cfg(target_os = "macos")]
    MacOs(mac::MacOs),
    #[cfg(test)]
    Fake(tests::Fake),
}

impl Session {
    pub fn name(&self) -> &str {
        struct Name;
        impl Visit for Name {
            type Out = &'static str;
            fn backend<B: Backend>(self, _: &B) -> &'static str {
                B::NAME
            }
            fn unsupported(self) -> &'static str {
                ""
            }
        }
        match self {
            Self::Unsupported(desktop) => desktop,
            _ => self.visit(Name),
        }
    }

    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unsupported(_))
    }

    /// Enabled displays, sorted left to right.
    pub fn monitors(&self) -> Result<Vec<Monitor>> {
        struct Monitors;
        impl Visit for Monitors {
            type Out = Result<Vec<Monitor>>;
            fn backend<B: Backend>(self, b: &B) -> Self::Out {
                b.monitors()
            }
            fn unsupported(self) -> Self::Out {
                Ok(Vec::new())
            }
        }
        self.visit(Monitors)
    }

    pub fn desktop(&self) -> Option<Wall<'_, Desktop>> {
        self.wall()
    }

    pub fn lock(&self) -> Option<Wall<'_, Lock>> {
        self.wall()
    }

    fn wall<S: Surface>(&self) -> Option<Wall<'_, S>> {
        struct Probe<S>(PhantomData<S>);
        impl<S: Surface> Visit for Probe<S> {
            type Out = Option<(Reach, bool)>;
            fn backend<B: Backend>(self, _: &B) -> Self::Out {
                S::ops(&B::CAPS).map(|ops| (ops.reach(), ops.read.is_some()))
            }
            fn unsupported(self) -> Self::Out {
                None
            }
        }
        let (reach, readable) = self.visit(Probe::<S>(PhantomData))?;
        Some(Wall { session: self, reach, readable, surface: PhantomData })
    }
}

/// A surface the active backend can set. Only [`Session`] makes these, and
/// only for surfaces the backend supports.
pub struct Wall<'a, S> {
    session: &'a Session,
    reach: Reach,
    readable: bool,
    surface: PhantomData<S>,
}

/// Runs one surface operation against the concrete backend.
enum WallOp<'a> {
    Each(&'a Images, &'a [Monitor]),
    One(&'a Path, &'a [Monitor]),
}

impl<S: Surface> Wall<'_, S> {
    pub fn reach(&self) -> Reach {
        self.reach
    }

    /// Set each monitor's image. On a [`Reach::Shared`] surface the primary
    /// monitor's image goes everywhere.
    pub fn set(&self, images: &Images, monitors: &[Monitor]) -> Result<Shown> {
        self.run(WallOp::Each(images, monitors))
    }

    /// Put `image` on every monitor.
    pub fn set_one(&self, image: &Path, monitors: &[Monitor]) -> Result<Shown> {
        self.run(WallOp::One(image, monitors))
    }

    /// What each monitor shows now, or `None` if the backend can't say.
    pub fn current(&self, monitors: &[Monitor]) -> Option<Result<Images>> {
        struct Read<'a, S>(&'a [Monitor], PhantomData<S>);
        impl<S: Surface> Visit for Read<'_, S> {
            type Out = Option<Result<Images>>;
            fn backend<B: Backend>(self, b: &B) -> Self::Out {
                S::ops(&B::CAPS)?.read.map(|read| read(b, self.0))
            }
            fn unsupported(self) -> Self::Out {
                None
            }
        }
        if !self.readable {
            return None;
        }
        self.session.visit(Read::<S>(monitors, PhantomData))
    }

    fn run(&self, op: WallOp<'_>) -> Result<Shown> {
        struct Run<'a, S>(WallOp<'a>, PhantomData<S>);
        impl<S: Surface> Visit for Run<'_, S> {
            type Out = Result<Shown>;
            fn backend<B: Backend>(self, b: &B) -> Self::Out {
                let ops = S::ops(&B::CAPS).context("surface not supported")?;
                match (self.0, ops.each) {
                    (WallOp::Each(images, monitors), Some(each)) => each(b, images, monitors),
                    (WallOp::Each(images, monitors), None) => {
                        let image = primary_image(images, monitors).context("nothing assigned to any monitor")?;
                        (ops.one)(b, image, monitors)
                    }
                    (WallOp::One(image, monitors), _) => (ops.one)(b, image, monitors),
                }
            }
            fn unsupported(self) -> Self::Out {
                bail!("no backend for this desktop")
            }
        }
        self.session.visit(Run::<S>(op, PhantomData))
    }
}

impl Wall<'_, Lock> {
    /// Set the lock screen the way the user prefers.
    pub fn apply(&self, images: &Images, monitors: &[Monitor], mode: LockMode) -> Result<Shown> {
        match mode {
            LockMode::PerMonitor => self.set(images, monitors),
            LockMode::Single => {
                let image = primary_image(images, monitors).context("nothing assigned to any monitor")?;
                self.set_one(image, monitors)
            }
        }
    }
}

/// The image for the primary monitor (or the leftmost), for surfaces that
/// can only show one image.
pub fn primary_image<'a>(images: &'a Images, monitors: &[Monitor]) -> Option<&'a PathBuf> {
    monitors
        .iter()
        .filter(|m| m.primary)
        .chain(monitors.iter())
        .find_map(|m| images.get(&m.name))
        .or_else(|| images.values().next())
}

#[cfg(test)]
mod tests {
    use super::{
        Backend, Caps, Desktop, Images, Lock, Ops, Reach, ReadBack, Session, SetEach, SetOne, Shown, primary_image,
    };
    use crate::config::LockMode;
    use crate::monitors::Monitor;
    use anyhow::Result;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    fn mon(name: &str, x: i32, primary: bool) -> Monitor {
        Monitor { name: name.into(), x, y: 0, width: 1920, height: 1080, px_width: 1920, px_height: 1080, primary }
    }

    #[test]
    fn primary_image_prefers_the_primary_then_leftmost() {
        let ms = [mon("A", 0, false), mon("B", 1920, true)];
        let mut images = Images::new();
        images.insert("A".into(), PathBuf::from("a.jpg"));
        assert_eq!(primary_image(&images, &ms), Some(&PathBuf::from("a.jpg")));
        images.insert("B".into(), PathBuf::from("b.jpg"));
        assert_eq!(primary_image(&images, &ms), Some(&PathBuf::from("b.jpg")));
    }

    /// Per-monitor desktop that can be read back; one shared lock image.
    #[derive(Default)]
    pub struct Fake(Mutex<Vec<String>>);

    impl Fake {
        fn log(&self, entry: String) -> Result<()> {
            self.0.lock().unwrap().push(entry);
            Ok(())
        }
    }

    impl SetOne<Desktop> for Fake {
        fn set_one(&self, image: &Path, _: &[Monitor]) -> Result<Shown> {
            self.log(format!("desktop one {}", image.display())).map(|()| None)
        }
    }

    impl SetEach<Desktop> for Fake {
        fn set_each(&self, images: &Images, _: &[Monitor]) -> Result<Shown> {
            self.log(format!("desktop each {}", images.len())).map(|()| None)
        }
    }

    impl ReadBack<Desktop> for Fake {
        fn current(&self, monitors: &[Monitor]) -> Result<Images> {
            Ok(monitors.iter().map(|m| (m.name.clone(), PathBuf::from("now.jpg"))).collect())
        }
    }

    impl SetOne<Lock> for Fake {
        fn set_one(&self, image: &Path, _: &[Monitor]) -> Result<Shown> {
            self.log(format!("lock one {}", image.display())).map(|()| None)
        }
    }

    impl Backend for Fake {
        const ID: &'static str = "fake";
        const NAME: &'static str = "Fake";
        const CAPS: Caps<Self> = Caps { desktop: Ops::each().read(), lock: Some(Ops::one()) };

        fn matches(_: &str) -> bool {
            false
        }

        fn connect() -> Result<Self> {
            Ok(Self::default())
        }

        fn monitors(&self) -> Result<Vec<Monitor>> {
            Ok(vec![mon("A", 0, false), mon("B", 1920, true)])
        }
    }

    // Checked by the compiler, not at test time.
    const _: () = assert!(matches!(Fake::CAPS.desktop.reach(), Reach::PerMonitor));

    #[test]
    fn walls_follow_the_trait_impls() {
        let session = Session::Fake(Fake::default());
        let ms = session.monitors().unwrap();
        let images: Images = [("A".into(), "a.jpg".into()), ("B".into(), "b.jpg".into())].into();

        let desktop = session.desktop().unwrap();
        assert_eq!(desktop.reach(), Reach::PerMonitor);
        assert_eq!(desktop.current(&ms).unwrap().unwrap().len(), 2);
        desktop.set(&images, &ms).unwrap();

        // Shared lock: the primary's image, whatever the preference.
        let lock = session.lock().unwrap();
        assert_eq!(lock.reach(), Reach::Shared);
        assert!(lock.current(&ms).is_none());
        lock.apply(&images, &ms, LockMode::PerMonitor).unwrap();

        let Session::Fake(fake) = &session else { unreachable!() };
        assert_eq!(*fake.0.lock().unwrap(), ["desktop each 2", "lock one b.jpg"]);
        assert_eq!(session.name(), "Fake");
    }

    #[test]
    fn unsupported_desktops_offer_no_walls() {
        let session = Session::Unsupported("Hyprland".into());
        assert!(!session.is_supported());
        assert_eq!(session.name(), "Hyprland");
        assert!(session.desktop().is_none() && session.lock().is_none());
        assert!(session.monitors().unwrap().is_empty());
    }
}
