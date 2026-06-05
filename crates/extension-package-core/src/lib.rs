use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

use chrono::{DateTime, Utc};
use flate2::{Compression, GzBuilder};
use frontend_forge_api::{
    CrdScope, CrdTablePageSpec, ExtensionChartsSpec, ExtensionDependencySpec,
    ExtensionMaintainerSpec, ExtensionProviderSpec, FrontendExtension,
    FrontendExtensionFrontendSpec, FrontendExtensionSourceType, MenuPlacement, PageType,
};
use frontend_forge_common::{CommonError, serializable_hash, sha256_hex};
use frontend_forge_manifest::{
    ManifestRenderError, ResolvedFrontendPage, resolve_frontend_extension_pages,
};
use include_dir::{Dir, include_dir};
use kube::ResourceExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use snafu::{OptionExt, ResultExt, Snafu};
use tar::{Builder as TarBuilder, Header};

mod charts;
mod files;
mod identity;
mod package;
mod roles;
mod templates;
mod types;

#[cfg(test)]
mod tests;

pub(crate) use charts::*;
pub(crate) use files::*;
pub use identity::{frontend_extension_package_name, frontend_extension_source_hash};
pub use package::build_extension_package;
pub(crate) use roles::*;
pub(crate) use templates::*;
pub use types::*;

static PACKAGE_TEMPLATE_DIR: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/../../template/test-fe-demo");
