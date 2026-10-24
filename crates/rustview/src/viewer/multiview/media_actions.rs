//! The action buttons of the hover bar (pure).
//!
//! Rust decides *which* buttons a hovered medium gets, in which order and whether each is enabled (a disabled button
//! explains why in its tooltip); the Slint action bar only draws them. A new action is one provider function appended to
//! `PROVIDERS`. Providers look at where the Multiview is shown and what it was opened from: a Multiview that follows the Browse selection offers sticky/remove/open-as-tab; a Multiview that
//! belongs to a tab (shown in the tab or in the fullscreen) offers remove/open-as-tab too, but no sticky (a tab has no
//! Browse selection to stay independent of). On the last medium remove turns into a close button that fits the host.

use super::follow::{DuplicateRefusal, duplicate_refusal};
use super::key_labels;
use super::model::TileKind;
use super::route::{HostMode, OriginKind};
use crate::icons::Icon;
use crate::viewer::edit::save_badge::SaveLook;
use rustview_core::settings::schema::MULTIVIEW_MAX_TILES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaActionId {
    Mute,
    Sticky,
    OpenAsTab,
    Remove,
    Close,
    OpenWith,
    Delete,
    Duplicate,
    /// Opens or closes the zoom slider of an image.
    Zoom,
    /// Opens or cancels the crop tool of a picture.
    Crop,
    /// Saves the edited picture over the original (or through the export dialog when its format cannot be written).
    Save,
    /// The arrow of the Save button: the drop-down with Save, Save As... and Discard Changes.
    SaveMenu,
    Undo,
    Redo,
}

/// What a click in the media bar window means: a button, an item of the drop-down menu that hangs below the bar, or a
/// movement of the zoom slider (which hangs there too).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarClick {
    Action(MediaActionId),
    /// The id of the clicked menu item (the application defines them; they never collide with the button codes).
    MenuItem(i32),
    /// The slider asks for this zoom, in tenths of a percent (1500 = 150%).
    ZoomTo(i32),
    /// The handle of the slider is at this position (0..=10000 of its length); Rust snaps it and turns it into a zoom.
    ZoomPos(i32),
    /// A step on the slider: the wheel over it, or its plus and minus buttons.
    ZoomStep {
        up: bool,
        shift: bool,
        ctrl: bool,
    },
    /// The reset (100%).
    ZoomReset,
    /// An item of the drop-down of the Save button.
    SaveMenuItem(SaveMenuItem),
}

/// The items of the drop-down of the Save button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveMenuItem {
    Save,
    SaveAs,
    Discard,
}

impl SaveMenuItem {
    pub const fn index(self) -> i32 {
        match self {
            Self::Save => 0,
            Self::SaveAs => 1,
            Self::Discard => 2,
        }
    }

    pub fn from_index(index: i32) -> Option<Self> {
        [Self::Save, Self::SaveAs, Self::Discard]
            .into_iter()
            .find(|i| i.index() == index)
    }

    /// The id the item has in the drop-down (`BarClick::SAVE_MENU` + index).
    pub const fn code(self) -> i32 {
        BarClick::SAVE_MENU + self.index()
    }
}

impl BarClick {
    /// Button codes are small numbers; menu items start here.
    const FIRST_MENU_ITEM: i32 = 1000;
    /// The codes of the zoom slider: `ZOOM_TO + permille`, `ZOOM_STEP + flags`, `ZOOM_RESET`.
    pub const ZOOM_TO: i32 = 1_000_000;
    pub const ZOOM_STEP: i32 = 2_000_000;
    pub const ZOOM_RESET: i32 = 3_000_000;
    pub const ZOOM_POS: i32 = 4_000_000;
    /// The items of the Save drop-down: `SAVE_MENU + SaveMenuItem::index`.
    pub const SAVE_MENU: i32 = 5_000_000;
    const MAX_ZOOM_PERMILLE: i32 = 999_999;
    pub const MAX_ZOOM_POS: i32 = 10_000;

    pub fn from_code(code: i32) -> Option<Self> {
        if code >= Self::SAVE_MENU {
            SaveMenuItem::from_index(code - Self::SAVE_MENU).map(Self::SaveMenuItem)
        } else if code >= Self::ZOOM_POS {
            Some(Self::ZoomPos((code - Self::ZOOM_POS).min(Self::MAX_ZOOM_POS)))
        } else if code >= Self::ZOOM_RESET {
            (code == Self::ZOOM_RESET).then_some(Self::ZoomReset)
        } else if code >= Self::ZOOM_STEP {
            let flags = code - Self::ZOOM_STEP;
            (flags < 8).then_some(Self::ZoomStep {
                up: flags & 1 != 0,
                shift: flags & 2 != 0,
                ctrl: flags & 4 != 0,
            })
        } else if code >= Self::ZOOM_TO {
            Some(Self::ZoomTo((code - Self::ZOOM_TO).min(Self::MAX_ZOOM_PERMILLE)))
        } else if code >= Self::FIRST_MENU_ITEM {
            Some(Self::MenuItem(code))
        } else {
            MediaActionId::from_code(code).map(Self::Action)
        }
    }

    /// The number that crosses the Slint boundary for a zoom click (the slider uses the same formulas).
    pub fn zoom_code(self) -> Option<i32> {
        match self {
            Self::ZoomTo(permille) => Some(Self::ZOOM_TO + permille.clamp(0, Self::MAX_ZOOM_PERMILLE)),
            Self::ZoomStep { up, shift, ctrl } => {
                Some(Self::ZOOM_STEP + i32::from(up) + 2 * i32::from(shift) + 4 * i32::from(ctrl))
            }
            Self::ZoomReset => Some(Self::ZOOM_RESET),
            Self::ZoomPos(position) => Some(Self::ZOOM_POS + position.clamp(0, Self::MAX_ZOOM_POS)),
            Self::Action(_) | Self::MenuItem(_) | Self::SaveMenuItem(_) => None,
        }
    }
}

impl MediaActionId {
    /// Stable number that crosses the Slint boundary.
    pub const fn code(self) -> i32 {
        match self {
            Self::Mute => 0,
            Self::Sticky => 1,
            Self::OpenAsTab => 2,
            Self::Remove => 3,
            Self::Close => 4,
            Self::OpenWith => 5,
            Self::Delete => 6,
            Self::Duplicate => 7,
            Self::Zoom => 8,
            Self::Crop => 9,
            Self::Save => 10,
            Self::SaveMenu => 11,
            Self::Undo => 12,
            Self::Redo => 13,
        }
    }

    pub fn from_code(code: i32) -> Option<Self> {
        [
            Self::Mute,
            Self::Sticky,
            Self::OpenAsTab,
            Self::Remove,
            Self::Close,
            Self::OpenWith,
            Self::Delete,
            Self::Duplicate,
            Self::Zoom,
            Self::Crop,
            Self::Save,
            Self::SaveMenu,
            Self::Undo,
            Self::Redo,
        ]
        .into_iter()
        .find(|id| id.code() == code)
    }
}

/// Everything a provider may look at; all of it is in-memory state of the hovered tile and its Multiview.
#[derive(Debug, Clone, Copy)]
pub struct MediaBarContext {
    pub host: HostMode,
    pub origin: OriginKind,
    pub tile_count: usize,
    pub kind: TileKind,
    /// `Some(muted)` while the tile has sound control (a playing video), else `None`.
    pub muted: Option<bool>,
    pub sticky: bool,
    /// The tile shows an error instead of its medium (the file is gone, the video could not be played).
    pub broken: bool,
    /// `Some(open)` for a picture that can be zoomed (an image that is shown), with the state of its slider; else `None`.
    pub zoom: Option<bool>,
    /// The edit buttons of a picture that is shown (`None` for a video or a picture that is still loading).
    pub edit: Option<EditBarState>,
}

/// What the edit buttons of a tile show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditBarState {
    /// `Ok(open)`: the crop tool can be used (and is open or not); `Err(tooltip)`: the button is there but disabled.
    pub crop: Result<bool, &'static str>,
    /// The Save button, while there is one.
    pub save: Option<SaveLook>,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// How a button is drawn beyond its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonLook {
    #[default]
    Normal,
    /// Draws attention (unsaved changes).
    Emphasized,
    /// Working (the file is being written).
    Busy,
    /// Done; `fading` once it is on its way out.
    Done { fading: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaActionSpec {
    pub id: MediaActionId,
    pub icon: Icon,
    pub tooltip: String,
    /// `Some(on)` for buttons that show a toggle state.
    pub active: Option<bool>,
    /// A disabled button is drawn dimmed and does nothing; its tooltip says why.
    pub enabled: bool,
    pub look: ButtonLook,
    /// The button has an arrow of its own that opens a drop-down (`MediaActionId::SaveMenu` for Save).
    pub split: bool,
}

impl MediaActionSpec {
    fn on(id: MediaActionId, icon: Icon, tooltip: impl Into<String>) -> Self {
        Self {
            id,
            icon,
            tooltip: tooltip.into(),
            active: None,
            enabled: true,
            look: ButtonLook::Normal,
            split: false,
        }
    }
}

type Provider = fn(&MediaBarContext) -> Option<MediaActionSpec>;

/// Order of the buttons in the bar, left to right.
const PROVIDERS: &[Provider] = &[
    mute,
    sticky,
    duplicate,
    open_as_tab,
    open_with,
    delete,
    zoom,
    crop,
    undo,
    redo,
    save,
    remove,
    close,
];

/// The bar exists in the fullscreen window and in the viewer tab; the tab-grid miniature is a picture.
fn interactive(ctx: &MediaBarContext) -> bool {
    matches!(ctx.host, HostMode::Fullscreen | HostMode::TabView)
}

/// Only a Multiview that follows the Browse selection can keep media independent of that selection.
fn is_workspace(ctx: &MediaBarContext) -> bool {
    ctx.host == HostMode::Fullscreen && ctx.origin == OriginKind::Browse
}

fn mute(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    let muted = ctx.muted.filter(|_| interactive(ctx))?;
    Some(MediaActionSpec::on(
        MediaActionId::Mute,
        if muted { Icon::VolumeOff } else { Icon::VolumeOn },
        if muted { "Unmute" } else { "Mute" },
    ))
}

fn sticky(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    is_workspace(ctx).then(|| MediaActionSpec {
        active: Some(ctx.sticky),
        ..MediaActionSpec::on(
            MediaActionId::Sticky,
            if ctx.sticky {
                Icon::MultiviewStickyOn
            } else {
                Icon::MultiviewSticky
            },
            if ctx.sticky {
                "Sticky: stays when the selection changes (click to release)"
            } else {
                "Make sticky: keep this medium when the selection changes"
            },
        )
    })
}

/// Shows the medium a second time, as a tile of its own next to this one. The button is there whenever it could ever work and
/// is disabled, with the reason as tooltip, while it cannot: the view is full, or the tile has nothing to show. The rule
/// itself lives in `follow::duplicate_refusal`, which the click goes through as well.
fn duplicate(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    if !interactive(ctx) {
        return None;
    }
    let refusal = duplicate_refusal(ctx.tile_count, ctx.broken, MULTIVIEW_MAX_TILES);
    let tooltip = match refusal {
        None => "Show this medium a second time (as a tile of its own next to this one)".to_string(),
        Some(DuplicateRefusal::Full) => format!(
            "Show this medium a second time (disabled: the view already shows the maximum of {MULTIVIEW_MAX_TILES} media, remove one first)"
        ),
        Some(DuplicateRefusal::Broken | DuplicateRefusal::UnknownTile) => {
            "Show this medium a second time (disabled: this medium cannot be shown)".to_string()
        }
    };
    Some(MediaActionSpec {
        enabled: refusal.is_none(),
        ..MediaActionSpec::on(MediaActionId::Duplicate, Icon::MultiviewDuplicate, tooltip)
    })
}

/// Only a view that is not a tab already (the fullscreen that follows the Browse view) can send a medium to a tab: the tab view and
/// the fullscreen of a tab are tabs themselves.
fn open_as_tab(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    (interactive(ctx) && ctx.origin == OriginKind::Browse).then(|| {
        MediaActionSpec::on(
            MediaActionId::OpenAsTab,
            Icon::OpenInBackgroundTab,
            "Open in a background tab",
        )
    })
}

fn open_with(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    interactive(ctx).then(|| MediaActionSpec::on(MediaActionId::OpenWith, Icon::OpenWith, "Open with..."))
}

/// Images only (mpv zooms videos itself): the button toggles the slider that hangs below the bar.
fn zoom(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    let open = ctx.zoom.filter(|_| interactive(ctx))?;
    Some(MediaActionSpec {
        active: Some(open),
        ..MediaActionSpec::on(
            MediaActionId::Zoom,
            Icon::ZoomIn,
            if open {
                "Hide the zoom slider"
            } else {
                "Zoom (also Ctrl + mouse wheel over the image)"
            },
        )
    })
}

/// Images only: opens the crop tool (the button stays lit while it is open) or cancels it. A picture that cannot be edited
/// (animated, several pages, edited in another tile) keeps the button, disabled, with the reason as tooltip.
fn crop(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    let edit = ctx.edit.filter(|_| interactive(ctx))?;
    let apply = match key_labels::of(ctx.host).apply_crop {
        key if key.is_empty() => "double click".to_owned(),
        key => format!("double click or {key}"),
    };
    Some(match edit.crop {
        Ok(open) => MediaActionSpec {
            active: Some(open),
            ..MediaActionSpec::on(
                MediaActionId::Crop,
                Icon::Crop,
                if open {
                    format!("Cancel cropping (Esc); {apply} applies the crop")
                } else {
                    format!("Crop: drag a rectangle, Shift keeps the ratio, {apply} applies")
                },
            )
        },
        Err(reason) => MediaActionSpec {
            enabled: false,
            ..MediaActionSpec::on(MediaActionId::Crop, Icon::Crop, reason)
        },
    })
}

fn undo(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    let edit = ctx.edit.filter(|e| interactive(ctx) && (e.can_undo || e.can_redo))?;
    Some(MediaActionSpec {
        enabled: edit.can_undo,
        ..MediaActionSpec::on(
            MediaActionId::Undo,
            Icon::Undo,
            key_labels::with_key("Undo the last change", &key_labels::of(ctx.host).undo),
        )
    })
}

fn redo(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    let edit = ctx.edit.filter(|e| interactive(ctx) && (e.can_undo || e.can_redo))?;
    Some(MediaActionSpec {
        enabled: edit.can_redo,
        ..MediaActionSpec::on(
            MediaActionId::Redo,
            Icon::Redo,
            key_labels::with_key("Redo", &key_labels::of(ctx.host).redo),
        )
    })
}

/// Only while there is something to save (and briefly after saving, fading out). Its arrow opens Save, Save As... and Discard.
fn save(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    let look = ctx.edit.filter(|_| interactive(ctx))?.save?;
    let (icon, tooltip, look) = match look {
        SaveLook::Emphasized => (
            Icon::Save,
            format!(
                "{}; the arrow offers Save As...",
                key_labels::with_key("Save the changes over the original", &key_labels::of(ctx.host).save)
            ),
            ButtonLook::Emphasized,
        ),
        SaveLook::Busy => (Icon::Loader, "Saving...".to_owned(), ButtonLook::Busy),
        SaveLook::Done { fading } => (Icon::Save, "Saved".to_owned(), ButtonLook::Done { fading }),
    };
    Some(MediaActionSpec {
        look,
        split: matches!(look, ButtonLook::Emphasized),
        ..MediaActionSpec::on(MediaActionId::Save, icon, tooltip)
    })
}

fn delete(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    interactive(ctx).then(|| MediaActionSpec::on(MediaActionId::Delete, Icon::Delete, "Delete file..."))
}

fn remove(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    (interactive(ctx) && ctx.tile_count > 1).then(|| {
        MediaActionSpec::on(
            MediaActionId::Remove,
            Icon::MultiviewRemove,
            if ctx.origin == OriginKind::Browse {
                "Remove from this view and deselect"
            } else {
                "Remove from this tab"
            },
        )
    })
}

/// The last remaining medium cannot be removed (the view is never empty), so the button closes what shows it instead:
/// the fullscreen, or the tab.
fn close(ctx: &MediaBarContext) -> Option<MediaActionSpec> {
    (interactive(ctx) && ctx.tile_count <= 1).then(|| {
        MediaActionSpec::on(
            MediaActionId::Close,
            Icon::Close,
            if ctx.host == HostMode::Fullscreen {
                "Close fullscreen"
            } else {
                "Close tab"
            },
        )
    })
}

pub fn media_actions_for(ctx: &MediaBarContext) -> Vec<MediaActionSpec> {
    PROVIDERS.iter().filter_map(|provider| provider(ctx)).collect()
}
