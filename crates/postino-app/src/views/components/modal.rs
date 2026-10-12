//! The frame every modal shares: an `overlay` card with the modal radius, a 1 px `border` and the
//! design's shadow token (`docs/design-system.md`, "Modals"), inside gpui-component's `Dialog`.
//!
//! `Dialog` paints a surface and a shadow of its own. That shadow is fixed (black at 10%, set
//! inside the open animation, so a `.shadow(..)` on the dialog is overwritten on every frame) and
//! does not show over the scrim. [`framed_dialog`] makes the dialog's own box transparent and
//! larger than the card by the room the card's shadow needs, and draws the card inside it, so the
//! shadow fits in the dialog's content clip and slides and fades in with the dialog.

use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::{v_flex, window_paddings};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::theme::PaletteExt;
use crate::theme::metrics::RADIUS_MODAL;

/// Room kept around the card for its shadow (offset y 12, blur 32): the shadow fades out about
/// 1.5 times the blur past the card's edges, shifted down by the offset.
const SHADOW_ROOM_TOP: f32 = 28.0;
const SHADOW_ROOM_BOTTOM: f32 = 52.0;
const SHADOW_ROOM_X: f32 = 40.0;

/// The radius for a card's child that paints its own background into the card's corners (the
/// Settings nav column, the command palette's list): the modal radius minus the card's border.
pub const MODAL_INNER_RADIUS: f32 = RADIUS_MODAL - 1.0;

/// Least distance `Dialog` keeps between its box and the window edges (gpui-component's
/// `spacing.lg`). A box that does not fit is shrunk to it.
const WINDOW_EDGE: f32 = 16.0;

/// Where the card goes vertically.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModalPlacement {
    /// Centered in the window: Settings and Keyboard shortcuts.
    Centered,
    /// The card's top at `fraction` of the window height, and at least `min` px down: the command
    /// palette, and the small dialogs at gpui-component's own default of a tenth.
    Top { fraction: f32, min: f32 },
}

impl ModalPlacement {
    /// gpui-component's default placement: a tenth of the window height from the top.
    pub const DEFAULT: Self = Self::Top {
        fraction: 0.1,
        min: 0.0,
    };
}

/// The dialog box for a card, worked out from the window's usable size.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ModalGeometry {
    /// Width of the dialog box, card plus side room.
    width: f32,
    /// Height of the dialog box, card plus top and bottom room. `None` for a card as tall as its
    /// content.
    height: Option<f32>,
    /// The box's distance from the top of the window's usable area.
    margin_top: f32,
    /// Room between the box and the card, top, bottom and each side.
    room_top: f32,
    room_bottom: f32,
    room_x: f32,
}

impl ModalGeometry {
    /// The box for a card of `card_width` x `card_height` in a usable area of `view_width` x
    /// `view_height`. A card that fits keeps its size and gets the full shadow room. On a smaller
    /// window the room shrinks first, then the card, never past [`WINDOW_EDGE`].
    fn new(
        view_width: f32,
        view_height: f32,
        card_width: f32,
        card_height: Option<f32>,
        placement: ModalPlacement,
    ) -> Self {
        let usable_width = (view_width - 2.0 * WINDOW_EDGE).max(0.0);
        let card_width = card_width.min(usable_width);
        let room_x = SHADOW_ROOM_X.min((usable_width - card_width) / 2.0);

        let usable_height = (view_height - 2.0 * WINDOW_EDGE).max(0.0);
        let (card_height, room_top, room_bottom) = match card_height {
            Some(height) => {
                let height = height.min(usable_height);
                let scale = ((usable_height - height) / (SHADOW_ROOM_TOP + SHADOW_ROOM_BOTTOM))
                    .clamp(0.0, 1.0);
                (
                    Some(height),
                    SHADOW_ROOM_TOP * scale,
                    SHADOW_ROOM_BOTTOM * scale,
                )
            }
            None => (None, SHADOW_ROOM_TOP, SHADOW_ROOM_BOTTOM),
        };
        let height = card_height.map(|card| card + room_top + room_bottom);

        let card_top = match placement {
            ModalPlacement::Centered => (view_height - card_height.unwrap_or(0.0)) / 2.0,
            ModalPlacement::Top { fraction, min } => (view_height * fraction).max(min),
        };
        let lowest = view_height - WINDOW_EDGE - height.unwrap_or(0.0);
        let margin_top = (card_top - room_top).min(lowest).max(WINDOW_EDGE);

        Self {
            width: card_width + 2.0 * room_x,
            height,
            margin_top,
            room_top,
            room_bottom,
            room_x,
        }
    }
}

/// Turns `dialog` into the shared modal frame around `build`'s content: a card of `width` and
/// `height` (`None`: as tall as its content) placed at `placement`. The dialog's own box loses
/// its fill, border, padding and close button, so a modal draws its own close button in its
/// header. Call it inside `open_dialog`'s builder, which runs on every render, so the frame
/// follows window resizes.
pub fn framed_dialog<F>(
    dialog: Dialog,
    width: f32,
    height: Option<f32>,
    placement: ModalPlacement,
    window: &Window,
    build: F,
) -> Dialog
where
    F: Fn(&mut Window, &mut App) -> AnyElement + 'static,
{
    let paddings = window_paddings(window);
    let viewport = window.viewport_size();
    let view_width: f32 = (viewport.width - paddings.left - paddings.right).into();
    let view_height: f32 = (viewport.height - paddings.top - paddings.bottom).into();
    let geometry = ModalGeometry::new(view_width, view_height, width, height, placement);

    let dialog = dialog
        .w(px(geometry.width))
        .margin_top(px(geometry.margin_top))
        .p_0()
        .border_0()
        .bg(transparent_black())
        .close_button(false);
    let dialog = match geometry.height {
        Some(height) => dialog.h(px(height)),
        None => dialog,
    };
    dialog.content(move |content, window, cx| {
        let palette = cx.palette();
        // `DialogContent` is `v_flex().flex_1()`, and a flex item's automatic minimum size is its
        // content's natural size: without `min_h_0()` on it and on the card, a tall modal grows
        // past its box instead of letting its own scrollable body (Settings, Keyboard shortcuts)
        // scroll.
        content
            .min_h_0()
            .pt(px(geometry.room_top))
            .pb(px(geometry.room_bottom))
            .px(px(geometry.room_x))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .bg(palette.overlay)
                    .rounded(px(RADIUS_MODAL))
                    .border_1()
                    .border_color(palette.border)
                    .shadow(palette.shadow.clone())
                    .child(build(window, cx)),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_that_fits_keeps_its_size_and_the_full_room() {
        let geometry =
            ModalGeometry::new(1600.0, 1000.0, 800.0, Some(720.0), ModalPlacement::Centered);
        assert_eq!(geometry.width, 800.0 + 2.0 * SHADOW_ROOM_X);
        assert_eq!(
            geometry.height,
            Some(720.0 + SHADOW_ROOM_TOP + SHADOW_ROOM_BOTTOM)
        );
        // The card, not the box, is centered.
        assert_eq!(
            geometry.margin_top + geometry.room_top,
            (1000.0 - 720.0) / 2.0
        );
    }

    #[test]
    fn a_short_window_shrinks_the_room_before_the_card() {
        // 760 tall leaves 728 between the edges: 8 px of room for a 720 px card.
        let geometry =
            ModalGeometry::new(1280.0, 760.0, 800.0, Some(720.0), ModalPlacement::Centered);
        assert_eq!(geometry.height, Some(728.0));
        assert!((geometry.room_top + geometry.room_bottom - 8.0).abs() < 0.001);
        assert_eq!(geometry.margin_top, WINDOW_EDGE);
    }

    #[test]
    fn a_window_shorter_than_the_card_shrinks_the_card_to_the_edges() {
        let geometry =
            ModalGeometry::new(1280.0, 600.0, 800.0, Some(720.0), ModalPlacement::Centered);
        assert_eq!(geometry.height, Some(600.0 - 2.0 * WINDOW_EDGE));
        assert_eq!(geometry.room_top, 0.0);
        assert_eq!(geometry.room_bottom, 0.0);
        assert_eq!(geometry.margin_top, WINDOW_EDGE);
    }

    #[test]
    fn a_narrow_window_shrinks_the_side_room_then_the_card() {
        let roomy = ModalGeometry::new(900.0, 1000.0, 800.0, None, ModalPlacement::DEFAULT);
        assert_eq!(roomy.room_x, (900.0 - 2.0 * WINDOW_EDGE - 800.0) / 2.0);
        assert_eq!(roomy.width, 900.0 - 2.0 * WINDOW_EDGE);

        let narrow = ModalGeometry::new(700.0, 1000.0, 800.0, None, ModalPlacement::DEFAULT);
        assert_eq!(narrow.room_x, 0.0);
        assert_eq!(narrow.width, 700.0 - 2.0 * WINDOW_EDGE);
    }

    #[test]
    fn a_top_placed_card_sits_at_its_fraction_with_a_floor() {
        let placement = ModalPlacement::Top {
            fraction: 0.14,
            min: 48.0,
        };
        let tall = ModalGeometry::new(1600.0, 1000.0, 560.0, None, placement);
        assert_eq!(tall.height, None);
        assert_eq!(tall.margin_top + tall.room_top, 140.0);

        let short = ModalGeometry::new(1600.0, 300.0, 560.0, None, placement);
        assert_eq!(short.margin_top + short.room_top, 48.0);
    }

    #[test]
    fn the_box_never_goes_above_the_window_edge() {
        let geometry = ModalGeometry::new(
            1600.0,
            1000.0,
            400.0,
            None,
            ModalPlacement::Top {
                fraction: 0.0,
                min: 0.0,
            },
        );
        assert_eq!(geometry.margin_top, WINDOW_EDGE);
    }
}
