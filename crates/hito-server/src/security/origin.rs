use axum::extract::Request;

pub fn allowed(request: &Request, origin: &str, additional_origins: &[String]) -> bool {
    if matches!(request.method().as_str(), "GET" | "HEAD" | "OPTIONS") {
        return true;
    }
    let origin_matches = request.headers().get("origin").is_none_or(|value| {
        value.to_str().is_ok_and(|value| {
            value == origin || additional_origins.iter().any(|origin| value == origin)
        })
    });
    let custom_header = request
        .headers()
        .get("x-hito-request")
        .is_some_and(|value| value == "1");
    let bearer = request.headers().get("authorization").is_some_and(|value| {
        value
            .to_str()
            .is_ok_and(|value| value.starts_with("Bearer "))
    });
    origin_matches && (custom_header || bearer)
}
