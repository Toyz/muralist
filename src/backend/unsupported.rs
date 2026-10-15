//! Fallback for desktops muralist doesn't know yet: reports nothing and
//! explains itself instead of failing in confusing ways.

use anyhow::{Result, bail};

use super::{Backend, Capabilities, Images};
use crate::monitors::Monitor;

pub(super) struct Unsupported {
    name: String,
}

impl Unsupported {
    pub(super) fn new(desktop: String) -> Self {
        let name = if desktop.is_empty() { "this desktop".to_owned() } else { desktop };
        Self { name }
    }
}

impl Backend for Unsupported {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    fn is_supported(&self) -> bool {
        false
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        Ok(Vec::new())
    }

    fn set_desktop(&self, _images: &Images, _monitors: &[Monitor]) -> Result<()> {
        bail!("muralist can't set wallpapers on {} yet", self.name)
    }
}
