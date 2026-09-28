pub mod color;
pub mod components;
pub mod highlight;
pub mod keys;
pub mod markdown;
pub mod styles;
pub mod views;
pub mod wrap;

pub struct RenderCtx<'a> {
    pub label_colors: &'a styles::LabelColors,
}
