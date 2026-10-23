fn main() -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string_pretty(&hito_server::openapi())?);
    Ok(())
}
