use std::{env, error::Error, fs, path::PathBuf};

use frontend_forge_api::FrontendExtension;
use frontend_forge_common::manifest_content_and_hash;
use frontend_forge_manifest::render_frontend_extension_manifest;

type DynError = Box<dyn Error + Send + Sync>;

fn main() -> Result<(), DynError> {
    let path = env::args().nth(1).map(PathBuf::from).ok_or(
        "usage: cargo run -p frontend-forge-manifest --example render_fe_manifest -- <fe.yaml>",
    )?;

    let fe_text = fs::read_to_string(path)?;
    let fe: FrontendExtension = serde_yaml::from_str(&fe_text)?;
    let manifest_value = render_frontend_extension_manifest(&fe)?;
    let (manifest_content, _) = manifest_content_and_hash(&manifest_value)?;

    println!("{manifest_content}");
    Ok(())
}
