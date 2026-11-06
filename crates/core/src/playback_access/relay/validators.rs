use reqwest::header::HeaderMap;

const MAX_VALIDATOR_BYTES: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioValidators {
    etag: Option<String>,
    last_modified: Option<String>,
}

impl AudioValidators {
    pub(crate) fn read(headers: &HeaderMap) -> Self {
        Self {
            etag: single(headers, "etag"),
            last_modified: single(headers, "last-modified"),
        }
    }

    pub(crate) fn still_describes(&self, current: &Self) -> bool {
        agrees(self.etag.as_deref(), current.etag.as_deref(), strong)
            && agrees(
                self.last_modified.as_deref(),
                current.last_modified.as_deref(),
                |_| true,
            )
    }
}

fn agrees(original: Option<&str>, current: Option<&str>, usable: fn(&str) -> bool) -> bool {
    match (original, current) {
        (None, _) => true,
        (Some(original), Some(current)) => usable(original) && original == current,
        (Some(_), None) => false,
    }
}

fn strong(etag: &str) -> bool {
    !etag.starts_with("W/")
}

fn single(headers: &HeaderMap, name: &'static str) -> Option<String> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() || value.is_empty() || value.len() > MAX_VALIDATOR_BYTES {
        return None;
    }
    Some(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(etag: Option<&str>, modified: Option<&str>) -> AudioValidators {
        let mut headers = HeaderMap::new();
        if let Some(etag) = etag {
            headers.insert("etag", etag.parse().unwrap());
        }
        if let Some(modified) = modified {
            headers.insert("last-modified", modified.parse().unwrap());
        }
        AudioValidators::read(&headers)
    }

    #[test]
    fn matching_strong_validators_describe_the_same_representation() {
        let first = with(Some("\"a\""), Some("Tue, 03 Nov 2026 10:00:00 GMT"));
        assert!(first.still_describes(&first.clone()));
        assert!(with(None, None).still_describes(&with(Some("\"b\""), None)));
    }

    #[test]
    fn a_changed_or_missing_validator_does_not() {
        let first = with(Some("\"a\""), None);
        assert!(!first.still_describes(&with(Some("\"b\""), None)));
        assert!(!first.still_describes(&with(None, None)));
        let dated = with(None, Some("Tue, 03 Nov 2026 10:00:00 GMT"));
        assert!(!dated.still_describes(&with(None, Some("Wed, 04 Nov 2026 10:00:00 GMT"))));
    }

    #[test]
    fn a_weak_entity_tag_never_supports_a_resume() {
        let weak = with(Some("W/\"a\""), None);
        assert!(!weak.still_describes(&weak.clone()));
    }
}
