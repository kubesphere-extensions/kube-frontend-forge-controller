use frontend_forge_api::{frontend_extension_crd, frontend_integration_crd};
use std::{env, error::Error, fs, path::PathBuf, process};

const FRONTEND_INTEGRATION_CRD_PATH: &str =
    "config/crd/bases/frontend-forge.kubesphere.io_frontendintegrations.yaml";
const FRONTEND_EXTENSION_CRD_PATH: &str =
    "config/crd/bases/frontend-forge.kubesphere.io_frontendextensions.yaml";

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match env::args().nth(1).as_deref() {
        Some("gen-crd") => gen_crd(),
        _ => {
            eprintln!("usage: cargo xtask gen-crd");
            process::exit(2);
        }
    }
}

fn gen_crd() -> Result<(), Box<dyn Error>> {
    write_crd(
        FRONTEND_INTEGRATION_CRD_PATH,
        serde_yaml::to_string(&frontend_integration_crd())?,
    )?;
    write_crd(
        FRONTEND_EXTENSION_CRD_PATH,
        serde_yaml::to_string(&frontend_extension_crd())?,
    )?;
    Ok(())
}

fn write_crd(relative_path: &str, rendered: String) -> Result<(), Box<dyn Error>> {
    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask workspace root")
        .join(relative_path);

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&output_path, rendered)?;
    println!("updated {}", output_path.display());
    Ok(())
}
