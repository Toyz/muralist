//! Drag a card straight onto a monitor in the Displays panel.

use std::path::PathBuf;

use gpui_kit::*;

use super::{CARD_INNER_R, Item, pal, radius};

/// What travels with the pointer while a card is dragged.
#[derive(Clone)]
pub(super) struct CardDrag {
    pub item: Item,
    pub thumb: Option<PathBuf>,
}

/// The little floating card under the cursor.
pub(super) struct DragGhost {
    thumb: Option<PathBuf>,
    /// Pointer offset within the source card, so the ghost centers on it.
    offset: Point<Pixels>,
}

pub(super) const GHOST_W: f32 = 168.;
pub(super) const GHOST_H: f32 = 105.;

impl DragGhost {
    pub(super) fn new(drag: &CardDrag, offset: Point<Pixels>) -> Self {
        Self { thumb: drag.thumb.clone(), offset }
    }
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = pal();
        div()
            .ml(self.offset.x - px(GHOST_W / 2.))
            .mt(self.offset.y - px(GHOST_H / 2.))
            .w(px(GHOST_W))
            .h(px(GHOST_H))
            .rounded_xl()
            .overflow_hidden()
            .border_2()
            .border_color(p.accent)
            .bg(p.surface2)
            .shadow_lg()
            .opacity(0.92)
            .when_some(self.thumb.clone(), |d, t| {
                d.child(radius(img(t).size_full().object_fit(ObjectFit::Cover), CARD_INNER_R))
            })
    }
}

use gpui_kit::prelude::FluentBuilder as _;
