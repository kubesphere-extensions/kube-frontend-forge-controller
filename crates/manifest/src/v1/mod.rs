use frontend_forge_api::{
    ColumnRenderType, ColumnSpec, CrdScope, CrdTablePageSpec, MenuNodeType, MenuPlacement,
    PageSpec, PageType,
};
use serde_json::{Map, Value, json};

use crate::{FrontendRenderInput, ManifestRenderError, ResolvedFrontendPage};

mod columns;
mod pages;
mod render;
mod resolve;

#[cfg(test)]
mod tests;

pub(crate) use columns::*;
pub(crate) use pages::*;
pub use render::render_v1_manifest;
pub(crate) use render::*;
pub use resolve::resolve_v1_pages;
pub(crate) use resolve::*;

const DEFAULT_MENU_ICON: &str = "GridDuotone";
