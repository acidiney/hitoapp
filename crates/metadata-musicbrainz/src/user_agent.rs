use crate::Error;

const MAX_CONTACT_BYTES: usize = 128;

pub(crate) fn user_agent(contact: Option<&str>) -> Result<String, Error> {
    let base = concat!("Hito/", env!("CARGO_PKG_VERSION"));
    let Some(contact) = contact else {
        return Ok(base.to_owned());
    };
    let acceptable = !contact.is_empty()
        && contact.len() <= MAX_CONTACT_BYTES
        && contact
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'(' | b')'));
    if !acceptable {
        return Err(Error::Configuration);
    }
    Ok(format!("{base} ({contact})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_contact_the_agent_names_only_the_application() {
        assert_eq!(
            user_agent(None).unwrap(),
            concat!("Hito/", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn a_configured_contact_follows_the_version_in_parentheses() {
        assert_eq!(
            user_agent(Some("test@example.invalid")).unwrap(),
            format!("Hito/{} (test@example.invalid)", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn a_contact_that_could_break_the_header_is_refused() {
        for contact in ["", "a b", "x)y", "line\nbreak", &"a".repeat(129)] {
            assert!(user_agent(Some(contact)).is_err(), "{contact:?}");
        }
    }
}
