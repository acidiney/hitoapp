use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Clone, Copy)]
pub enum Tone {
    Success,
    Notice,
    Failure,
    Locked,
    Working,
}

impl Tone {
    fn name(self) -> &'static str {
        match self {
            Tone::Success => "success",
            Tone::Notice => "notice",
            Tone::Failure => "failure",
            Tone::Locked => "locked",
            Tone::Working => "working",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tone::Success => "<path d=\"M6 12.5l4 4 8-9\"/>",
            Tone::Notice => "<path d=\"M12 7v6\"/><path d=\"M12 16.8v.2\"/>",
            Tone::Failure => "<path d=\"M7.5 7.5l9 9\"/><path d=\"M16.5 7.5l-9 9\"/>",
            Tone::Locked => {
                "<rect x=\"6.5\" y=\"11\" width=\"11\" height=\"8\" rx=\"2\"/><path d=\"M9 11V8.5a3 3 0 016 0V11\"/>"
            }
            Tone::Working => "",
        }
    }
}

pub struct CallbackPage {
    status: StatusCode,
    tone: Tone,
    title: &'static str,
    message: &'static str,
    refresh_after_one_second: bool,
    return_link: bool,
}

impl CallbackPage {
    pub fn outcome(
        status: StatusCode,
        tone: Tone,
        title: &'static str,
        message: &'static str,
    ) -> Self {
        Self {
            status,
            tone,
            title,
            message,
            refresh_after_one_second: false,
            return_link: true,
        }
    }

    pub fn without_return_link(mut self) -> Self {
        self.return_link = false;
        self
    }

    pub fn retry() -> Self {
        Self {
            status: StatusCode::OK,
            tone: Tone::Working,
            title: "Finishing your connection",
            message: "Hito is completing your provider connection. This takes a moment.",
            refresh_after_one_second: true,
            return_link: true,
        }
    }

    fn render(&self) -> String {
        let refresh = if self.refresh_after_one_second {
            "<meta http-equiv=\"refresh\" content=\"1\">"
        } else {
            ""
        };
        let action = if self.refresh_after_one_second || !self.return_link {
            ""
        } else {
            "<a class=\"button\" href=\"/\">Return to Hito</a>"
        };
        let tone = self.tone.name();
        let icon = self.tone.icon();
        let title = self.title;
        let message = self.message;
        format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"color-scheme\" content=\"light dark\">{refresh}<title>Hito</title><style>{STYLE}</style></head><body><main class=\"card\" data-tone=\"{tone}\"><div class=\"brand\"><span class=\"mark\" aria-hidden=\"true\"><i class=\"ring library\"></i><i class=\"ring available\"></i><i class=\"ring catalog\"></i></span><span class=\"word\">hito</span></div><div class=\"badge\" aria-hidden=\"true\"><svg viewBox=\"0 0 24 24\" width=\"28\" height=\"28\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{icon}</svg><span class=\"spinner\"></span></div><h1>{title}</h1><p>{message}</p>{action}</main></body></html>"
        )
    }
}

impl IntoResponse for CallbackPage {
    fn into_response(self) -> Response {
        (
            self.status,
            [
                ("content-type", "text/html; charset=utf-8"),
                ("cache-control", "no-store"),
                ("referrer-policy", "no-referrer"),
            ],
            self.render(),
        )
            .into_response()
    }
}

const STYLE: &str = "\
:root{color-scheme:light dark;--bg:#fbfbfa;--surface:#fff;--line:#e6e5e1;--ink:#141414;--on-ink:#fff;--ink-hover:#3a3a37;--muted:#5f5e5a;--action:#2f4bff;--success:#0f7a4f;--warning:#9a5b00;--warning-bg:#fff4e5;--error:#b5412b;--error-bg:#fdeeea;--success-bg:#e6f4ec;--subtle:#eeeeea}\
@media(prefers-color-scheme:dark){:root{--bg:#111110;--surface:#1b1b19;--line:#2e2e2b;--ink:#f2f1ed;--on-ink:#111110;--ink-hover:#fff;--muted:#a3a29c;--action:#6f86ff;--success:#4cc38a;--warning:#e8a23d;--warning-bg:#2a2110;--error:#ef7f68;--error-bg:#2d1713;--success-bg:#10281d;--subtle:#262624}}\
*{box-sizing:border-box}\
body{margin:0;min-height:100vh;display:grid;place-items:center;padding:16px;background:var(--bg);color:var(--ink);font:15px/1.5 \"Geist Variable\",Geist,system-ui,-apple-system,\"Segoe UI\",sans-serif}\
.card{width:100%;max-width:440px;padding:32px 28px;border:1px solid var(--line);border-radius:16px;background:var(--surface);text-align:center}\
.brand{display:flex;align-items:center;justify-content:center;gap:10px;margin-bottom:28px}\
.mark{display:inline-flex;align-items:center;gap:3px}\
.ring{display:inline-block;width:10px;height:10px;border-radius:50%}\
.ring.library{background:var(--ink)}\
.ring.available{border:2px solid var(--action)}\
.ring.catalog{border:2px dashed var(--muted)}\
.word{font-size:24px;font-weight:800;line-height:1;letter-spacing:-.05em}\
.badge{display:grid;place-items:center;width:56px;height:56px;margin:0 auto 16px;border-radius:50%;background:var(--subtle);color:var(--muted)}\
[data-tone=success] .badge{background:var(--success-bg);color:var(--success)}\
[data-tone=notice] .badge,[data-tone=locked] .badge{background:var(--warning-bg);color:var(--warning)}\
[data-tone=failure] .badge{background:var(--error-bg);color:var(--error)}\
.spinner{display:none}\
[data-tone=working] svg{display:none}\
[data-tone=working] .spinner{display:block;width:24px;height:24px;border:3px solid var(--line);border-top-color:var(--action);border-radius:50%;animation:spin .9s linear infinite}\
@keyframes spin{to{transform:rotate(360deg)}}\
@media(prefers-reduced-motion:reduce){.spinner{animation:none!important}}\
h1{margin:0 0 8px;font-size:22px;font-weight:800;line-height:1.25;letter-spacing:-.02em}\
p{margin:0;color:var(--muted)}\
.button{display:inline-flex;align-items:center;justify-content:center;min-height:44px;margin-top:24px;padding:0 22px;border-radius:12px;background:var(--ink);color:var(--on-ink);font-weight:700;text-decoration:none}\
.button:hover{background:var(--ink-hover)}\
.button:focus-visible{outline:2px solid var(--action);outline-offset:2px}\
@media(max-width:480px){body{padding:12px}.card{padding:28px 20px}.button{display:flex;width:100%}}";
