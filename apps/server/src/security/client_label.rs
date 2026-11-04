use axum::http::{HeaderMap, header};

const UNKNOWN: &str = "Unknown browser";
const MAX_AGENT_BYTES: usize = 1024;

const BROWSERS: &[(&[&str], &str)] = &[
    (&["Edg/", "EdgA/", "EdgiOS/", "Edge/"], "Edge"),
    (&["OPR/", "Opera"], "Opera"),
    (&["SamsungBrowser/"], "Samsung Internet"),
    (&["Firefox/", "FxiOS/"], "Firefox"),
    (&["CriOS/", "Chrome/", "Chromium/"], "Chrome"),
    (&["Safari/"], "Safari"),
];

const SYSTEMS: &[(&[&str], &str)] = &[
    (&["iPhone"], "iPhone"),
    (&["iPad"], "iPad"),
    (&["Android"], "Android"),
    (&["CrOS"], "ChromeOS"),
    (&["Windows"], "Windows"),
    (&["Macintosh", "Mac OS X"], "macOS"),
    (&["Linux", "X11"], "Linux"),
];

pub fn label(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .map_or_else(|| UNKNOWN.to_owned(), |value| describe(&String::from_utf8_lossy(value.as_bytes())))
}

fn family(agent: &str, table: &[(&[&str], &'static str)]) -> Option<&'static str> {
    table
        .iter()
        .find(|(markers, _)| markers.iter().any(|marker| agent.contains(marker)))
        .map(|(_, name)| *name)
}

fn describe(agent: &str) -> String {
    if agent.len() > MAX_AGENT_BYTES {
        return UNKNOWN.to_owned();
    }
    if agent.contains("HitoDesktop/") {
        return "Hito desktop".to_owned();
    }
    match (family(agent, BROWSERS), family(agent, SYSTEMS)) {
        (Some(browser), Some(system)) => format!("{browser} on {system}"),
        (Some(browser), None) => browser.to_owned(),
        (None, Some(system)) => format!("{UNKNOWN} on {system}"),
        (None, None) => UNKNOWN.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX_MAC: &str =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:133.0) Gecko/20100101 Firefox/133.0";
    const SAFARI_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15";
    const CHROME_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
    const EDGE_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0";
    const OPERA_LINUX: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36 OPR/105.0.0.0";
    const SAFARI_IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1";
    const CHROME_IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/120.0.6099.119 Mobile/15E148 Safari/604.1";
    const FIREFOX_IPAD: &str = "Mozilla/5.0 (iPad; CPU OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/121.0 Mobile/15E148 Safari/605.1.15";
    const CHROME_ANDROID: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.6099.144 Mobile Safari/537.36";
    const SAMSUNG_ANDROID: &str = "Mozilla/5.0 (Linux; Android 13; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/23.0 Chrome/115.0.0.0 Mobile Safari/537.36";
    const FIREFOX_ANDROID: &str =
        "Mozilla/5.0 (Android 14; Mobile; rv:121.0) Gecko/121.0 Firefox/121.0";
    const CHROME_CHROMEOS: &str = "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
    const DESKTOP: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) HitoDesktop/0.1.0";

    #[test]
    fn major_browsers_name_the_family_and_the_system() {
        for (agent, expected) in [
            (FIREFOX_MAC, "Firefox on macOS"),
            (SAFARI_MAC, "Safari on macOS"),
            (CHROME_WINDOWS, "Chrome on Windows"),
            (EDGE_WINDOWS, "Edge on Windows"),
            (OPERA_LINUX, "Opera on Linux"),
            (CHROME_CHROMEOS, "Chrome on ChromeOS"),
        ] {
            assert_eq!(describe(agent), expected, "{agent}");
        }
    }

    #[test]
    fn phones_and_tablets_name_the_device_family() {
        for (agent, expected) in [
            (SAFARI_IPHONE, "Safari on iPhone"),
            (CHROME_IPHONE, "Chrome on iPhone"),
            (FIREFOX_IPAD, "Firefox on iPad"),
            (CHROME_ANDROID, "Chrome on Android"),
            (SAMSUNG_ANDROID, "Samsung Internet on Android"),
            (FIREFOX_ANDROID, "Firefox on Android"),
        ] {
            assert_eq!(describe(agent), expected, "{agent}");
        }
    }

    #[test]
    fn the_desktop_application_has_its_own_label() {
        assert_eq!(describe(DESKTOP), "Hito desktop");
    }

    #[test]
    fn unknown_clients_keep_whatever_part_is_known() {
        assert_eq!(describe("curl/8.4.0"), "Unknown browser");
        assert_eq!(describe("Mozilla/5.0 (Windows NT 10.0)"), "Unknown browser on Windows");
        assert_eq!(describe("Firefox/120.0"), "Firefox");
    }

    #[test]
    fn empty_missing_garbage_and_oversized_input_are_unknown() {
        assert_eq!(describe(""), "Unknown browser");
        assert_eq!(describe("   "), "Unknown browser");
        assert_eq!(describe("\u{0}\u{1}\u{ff}\u{fffd}{}[]<script>"), "Unknown browser");
        assert_eq!(label(&HeaderMap::new()), "Unknown browser");
        let oversized = format!("{FIREFOX_MAC} {}", "x".repeat(100_000));
        assert_eq!(describe(&oversized), "Unknown browser");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_bytes(b"Mozilla/5.0 (X11; Linux x86_64) Firefox/133.0 \xe9").unwrap(),
        );
        assert_eq!(label(&headers), "Firefox on Linux");
    }

    #[test]
    fn a_label_never_contains_any_raw_input_beyond_family_names() {
        let agent = format!("{FIREFOX_MAC} 203.0.113.9 private-sentinel");
        let label = describe(&agent);
        assert_eq!(label, "Firefox on macOS");
        assert!(!label.contains("203.0.113.9") && !label.contains("sentinel"));
    }
}
