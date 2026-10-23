mod runtime;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    runtime::run().await
}
