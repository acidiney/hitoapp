use super::RuntimeResult;
use url::Url;

pub(super) fn parse_additional_origins(value: &str) -> RuntimeResult<Vec<String>> {
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "HITO_ADDITIONAL_ORIGINS must contain at most 16 comma-separated exact HTTP(S) origins, with at most 512 bytes per origin and 4096 bytes total",
        )
    };
    if value.len() > 4096 {
        return Err(invalid().into());
    }
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let entries: Vec<_> = value.split(',').map(str::trim).collect();
    if entries.len() > 16 {
        return Err(invalid().into());
    }
    let mut origins = Vec::new();
    for origin in entries {
        let valid = origin.len() <= 512
            && Url::parse(origin).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty()
                    && url.password().is_none()
                    && !origin.contains('*')
                    && url.origin().ascii_serialization() == origin
            });
        if !valid {
            return Err(invalid().into());
        }
        if !origins.iter().any(|existing| existing == origin) {
            origins.push(origin.to_owned());
        }
    }
    Ok(origins)
}

pub(super) fn validate_combined_origins(
    primary: &str,
    additional_origins: &[String],
) -> RuntimeResult<()> {
    if Url::parse(primary).is_ok_and(|url| url.scheme() == "https")
        && additional_origins
            .iter()
            .any(|origin| origin.starts_with("http://"))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "HITO_ADDITIONAL_ORIGINS must use HTTPS when HITO_PUBLIC_ORIGIN uses HTTPS",
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_additional_origins, validate_combined_origins};

    #[test]
    fn https_primary_rejects_http_aliases_that_cannot_retain_secure_sessions() {
        for alias in ["http://192.168.1.50:8080", "http://localhost:8080"] {
            let origins = parse_additional_origins(alias).unwrap();
            let error = validate_combined_origins("https://hito.example", &origins).unwrap_err();
            assert_eq!(
                error.to_string(),
                "HITO_ADDITIONAL_ORIGINS must use HTTPS when HITO_PUBLIC_ORIGIN uses HTTPS"
            );
        }
        let origins = parse_additional_origins("https://lan.hito.example").unwrap();
        assert!(validate_combined_origins("https://hito.example", &origins).is_ok());
        let origins = parse_additional_origins("http://192.168.1.50:8080").unwrap();
        assert!(validate_combined_origins("http://localhost:8080", &origins).is_ok());
        assert!(validate_combined_origins("https://hito.example", &[]).is_ok());
    }

    #[test]
    fn additional_origins_are_empty_by_default_and_accept_exact_http_origins() {
        assert!(parse_additional_origins("").unwrap().is_empty());
        assert!(parse_additional_origins("  ").unwrap().is_empty());
        assert_eq!(
            parse_additional_origins(
                "http://192.168.1.50:8080, https://hito.example, http://[::1]:8080, https://hito.example"
            )
            .unwrap(),
            [
                "http://192.168.1.50:8080",
                "https://hito.example",
                "http://[::1]:8080",
            ]
        );
    }

    #[test]
    fn additional_origins_reject_non_origins_and_unbounded_lists() {
        for value in [
            "*",
            "null",
            "ftp://hito.example",
            "https://*.hito.example",
            "https://user:password@hito.example",
            "https://hito.example/",
            "https://hito.example/path",
            "https://hito.example?query=value",
            "https://hito.example#fragment",
            "https://hito.example:99999",
            "https://HITO.example",
            "https://hito.example:443",
            "http://192.168.001.050:8080",
            "https://hito.example,",
            ",https://hito.example",
        ] {
            assert!(parse_additional_origins(value).is_err(), "{value}");
        }
        assert!(parse_additional_origins(&vec!["https://hito.example"; 17].join(",")).is_err());
        assert!(parse_additional_origins(&format!("https://{}", "a".repeat(513))).is_err());
        assert!(parse_additional_origins(&" ".repeat(4097)).is_err());
    }
}
