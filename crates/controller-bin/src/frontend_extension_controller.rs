#[tokio::main]
async fn main() -> Result<(), frontend_forge_controller::Error> {
    frontend_forge_controller::run_fe_controller().await
}
