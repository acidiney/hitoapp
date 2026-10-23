use serde_json::Value;

pub fn render(page: &Value) -> String {
    let encoded = page
        .to_string()
        .replace('/', "\\/")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Hito</title><link rel="stylesheet" href="/assets/app.css"><script type="module" src="/assets/app.js"></script></head><body><script data-page="app" type="application/json">{encoded}</script><div id="app"></div></body></html>"#
    )
}
