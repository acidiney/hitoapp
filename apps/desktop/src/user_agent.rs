#[cfg(target_os = "macos")]
const PLATFORM: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
#[cfg(target_os = "windows")]
const PLATFORM: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko)";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const PLATFORM: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";

pub fn user_agent() -> String {
    format!("{PLATFORM} HitoDesktop/{}", env!("CARGO_PKG_VERSION"))
}
