use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerAddress {
    typed_scheme: Option<&'static str>,
    host: String,
    port: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressProblem {
    Empty,
    NotAnAddress,
}

impl ServerAddress {
    pub fn parse(typed: &str) -> Result<Self, AddressProblem> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Err(AddressProblem::Empty);
        }
        if typed.chars().any(char::is_whitespace) {
            return Err(AddressProblem::NotAnAddress);
        }
        let lower = typed.to_ascii_lowercase();
        let (typed_scheme, rest) = match lower.split_once("://") {
            Some(("https", rest)) => (Some("https"), rest),
            Some(("http", rest)) => (Some("http"), rest),
            Some(_) => return Err(AddressProblem::NotAnAddress),
            None => (None, lower.as_str()),
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.is_empty() || authority.contains('@') {
            return Err(AddressProblem::NotAnAddress);
        }
        let parsed = Url::parse(&format!("http://{authority}/"))
            .map_err(|_| AddressProblem::NotAnAddress)?;
        let host = parsed
            .host_str()
            .filter(|host| !host.is_empty())
            .ok_or(AddressProblem::NotAnAddress)?
            .to_owned();
        let port = match (typed_scheme, parsed.port()) {
            (Some("https"), Some(443)) => None,
            (_, port) => port,
        };
        Ok(Self {
            typed_scheme,
            host,
            port,
        })
    }

    pub fn candidates(&self) -> Vec<Url> {
        let schemes: &[&str] = match self.typed_scheme {
            Some(scheme) => &[scheme][..],
            None => &["https", "http"],
        };
        schemes
            .iter()
            .filter_map(|scheme| Url::parse(&format!("{scheme}://{}/", self.authority())).ok())
            .collect()
    }

    pub fn shown(&self) -> String {
        match self.typed_scheme {
            Some(scheme) => format!("{scheme}://{}", self.authority()),
            None => self.authority(),
        }
    }

    fn authority(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{port}", self.host),
            None => self.host.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origins(typed: &str) -> Vec<String> {
        ServerAddress::parse(typed)
            .unwrap()
            .candidates()
            .iter()
            .map(|url| url.origin().ascii_serialization())
            .collect()
    }

    #[test]
    fn an_address_without_a_scheme_tries_https_first_then_http() {
        assert_eq!(
            origins("hito.home:8080"),
            ["https://hito.home:8080", "http://hito.home:8080"]
        );
    }

    #[test]
    fn a_typed_scheme_is_used_exactly_as_typed() {
        assert_eq!(
            origins("http://192.168.1.20:8080"),
            ["http://192.168.1.20:8080"]
        );
        assert_eq!(
            origins("https://music.example.org"),
            ["https://music.example.org"]
        );
    }

    #[test]
    fn anything_after_the_host_and_port_is_dropped_and_shown() {
        let address = ServerAddress::parse("https://music.example.org/library?tab=albums").unwrap();
        assert_eq!(address.shown(), "https://music.example.org");
        assert_eq!(
            origins("https://music.example.org/library?tab=albums"),
            ["https://music.example.org"]
        );
    }

    #[test]
    fn the_default_https_port_is_the_same_server() {
        assert_eq!(
            ServerAddress::parse("https://music.example.org:443").unwrap(),
            ServerAddress::parse("https://music.example.org").unwrap()
        );
    }

    #[test]
    fn an_ipv6_address_needs_its_square_brackets() {
        assert_eq!(
            origins("[fd00::12]:8080"),
            ["https://[fd00::12]:8080", "http://[fd00::12]:8080"]
        );
        assert_eq!(
            ServerAddress::parse("fd00::12"),
            Err(AddressProblem::NotAnAddress)
        );
    }

    #[test]
    fn addresses_hito_cannot_use_are_refused() {
        for typed in [
            "hito home:8080",
            "https://owner:secret@music.example.org",
            "ftp://music.example.org",
            "file:///etc/passwd",
            "https://",
        ] {
            assert_eq!(
                ServerAddress::parse(typed),
                Err(AddressProblem::NotAnAddress),
                "{typed}"
            );
        }
        assert_eq!(ServerAddress::parse("   "), Err(AddressProblem::Empty));
    }

    #[test]
    fn surrounding_spaces_and_letter_case_do_not_make_a_different_server() {
        assert_eq!(
            ServerAddress::parse("  HTTPS://Music.Example.org  ").unwrap(),
            ServerAddress::parse("https://music.example.org").unwrap()
        );
    }
}
