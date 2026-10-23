use hito_testkit::logs::LogCapture;

fn open_workflow_span() -> tracing::Span {
    tracing::debug_span!("owned_workflow")
}

#[test]
fn span_callsite_first_used_without_a_dispatcher_stays_enabled_for_a_capture() {
    let capture = LogCapture::default();
    let dispatch = capture.dispatch_with_spans("trace");
    let _scoped = tracing::dispatcher::set_default(&dispatch);

    std::thread::spawn(open_workflow_span).join().unwrap();

    let span = open_workflow_span();
    assert!(
        !span.is_disabled(),
        "a callsite first used on a thread without the capture must not disable the capture's spans"
    );
    let _entered = span.enter();
    tracing::debug!("inside the span");
    let events = capture.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["span"]["name"], "owned_workflow");
}
