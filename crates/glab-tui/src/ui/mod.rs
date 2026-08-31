pub mod components;
pub mod keys;
pub mod markdown;
pub mod styles;
pub mod views;

/// Shared rendering context passed to all view/component render functions.
/// Add fields here for any server-derived or global state needed during rendering.
pub struct RenderCtx<'a> {
    pub label_colors: &'a styles::LabelColors,
}
