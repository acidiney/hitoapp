use hito_testkit::logs::{LogCapture, trace_filter};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Capture(LogCapture);

impl Capture {
    #[allow(dead_code)]
    pub fn assert_private_requests(&self, forbidden: &[&str], statuses: &[u16]) {
        let text = self.0.text();
        for secret in forbidden {
            assert!(!text.contains(secret), "logs exposed a private canary");
        }
        let events: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("logs must be JSON events"))
            .collect();
        let completed: Vec<_> = events
            .iter()
            .filter(|event| event["fields"]["event"] == "real_debrid_plugin_request_finished")
            .collect();
        assert_eq!(
            completed.len(),
            statuses.len(),
            "every request must have one completion"
        );
        let mut requests = std::collections::HashSet::new();
        for (event, status) in completed.into_iter().zip(statuses) {
            let fields = &event["fields"];
            assert_eq!(fields["status"].as_u64(), Some(u64::from(*status)));
            assert_eq!(
                fields["outcome"],
                if *status < 400 { "success" } else { "rejected" }
            );
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let request =
                contextual_text(event, "request_id").expect("request correlation is required");
            let request = Uuid::parse_str(request).expect("request UUID must parse");
            assert!(
                !request.is_nil() && requests.insert(request),
                "request correlations must be unique"
            );
            let operation =
                contextual_text(event, "operation").expect("safe operation is required");
            assert!(["search", "detail", "sources", "audio"].contains(&operation));
        }
    }

    pub fn dispatch(&self) -> tracing::Dispatch {
        self.0.dispatch(&trace_filter(&[
            "hito_plugin_real_debrid",
            "hito_debrid_real_debrid",
            "hito_remote_audio",
        ]))
    }

    pub fn assert_private_completion(&self, forbidden: &[&str], length: u64) {
        let text = self.0.text();
        for secret in forbidden {
            assert!(!text.contains(secret), "logs exposed a private canary");
        }
        let events: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("logs must be JSON events"))
            .collect();
        let service: Vec<_> = events
            .iter()
            .filter(|event| {
                event["target"]
                    .as_str()
                    .is_some_and(|target| target.starts_with("hito_plugin_real_debrid"))
                    && event["fields"]["elapsed_ms"].as_u64().is_some()
            })
            .collect();
        assert!(!service.is_empty(), "service workflow logging was inactive");
        for event in service {
            let fields = &event["fields"];
            let outcome = fields["outcome"]
                .as_str()
                .expect("safe outcome is required");
            assert!(["success", "unauthorized", "complete", "ready"].contains(&outcome));
            let operation =
                contextual_text(event, "operation").expect("safe operation is required");
            assert!(["manifest", "search", "detail", "sources", "audio"].contains(&operation));
            let request = contextual_text(event, "request_id")
                .expect("service completion must retain its request UUID");
            assert!(
                !Uuid::parse_str(request)
                    .expect("request UUID must parse")
                    .is_nil()
            );
        }
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event["fields"]["event"] == "remote_audio_body_finished"
                        && event["fields"]["outcome"] == "complete"
                        && event["fields"]["bytes"].as_u64() == Some(length)
                })
                .count(),
            1,
            "the actual adapter body must record exactly one full completion"
        );
    }
}

fn contextual_text<'a>(event: &'a Value, field: &str) -> Option<&'a str> {
    event["fields"][field].as_str().or_else(|| {
        std::iter::once(&event["span"])
            .chain(event["spans"].as_array().into_iter().flatten())
            .find_map(|span| span[field].as_str())
    })
}
