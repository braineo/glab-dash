//! The terminal UI: [`app`] owns the state, [`cmd`] describes the side effects
//! it emits, [`keybindings`] drives dispatch and help alike, [`ui`] paints, and
//! [`run`] turns the crank.

pub mod app;
pub mod cmd;
pub mod keybindings;
pub mod run;
pub mod ui;
