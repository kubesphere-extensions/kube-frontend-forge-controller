use frontend_forge_api::{frontend_extension_crd, frontend_integration_crd};

fn main() -> Result<(), serde_yaml::Error> {
    let fi = frontend_integration_crd();
    let fe = frontend_extension_crd();
    // JSBundle is a third-party CRD (extensions.kubesphere.io) and is not generated
    // here.
    println!("{}", serde_yaml::to_string(&fi)?);
    println!("---");
    println!("{}", serde_yaml::to_string(&fe)?);
    Ok(())
}
