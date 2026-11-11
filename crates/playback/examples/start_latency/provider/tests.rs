use super::*;
use std::os::unix::net::UnixStream;

#[test]
fn malformed_protocol_output_is_a_terminal_failure() {
    let (input, mut output) = UnixStream::pair().unwrap();
    let writer = std::thread::spawn(move || output.write_all(b"not-json\n").unwrap());
    let (sender, _received) = mpsc::sync_channel(16);
    let result = read_events(input, sender);
    writer.join().unwrap();
    assert!(result.is_err(), "malformed output was silently discarded");
}
#[test]
fn oversized_protocol_line_is_rejected() {
    let (input, mut output) = UnixStream::pair().unwrap();
    let writer = std::thread::spawn(move || {
        let mut line = b"{\"event\":\"ack\",\"padding\":\"".to_vec();
        line.extend(vec![b'x'; 8192]);
        line.extend_from_slice(b"\"}\n");
        output.write_all(&line).unwrap();
    });
    let (sender, _received) = mpsc::sync_channel(16);
    let result = read_events(input, sender);
    writer.join().unwrap();
    assert!(result.is_err(), "unbounded protocol line was accepted");
}

#[test]
fn unsolicited_output_cannot_grow_an_unbounded_queue() {
    let (input, mut output) = UnixStream::pair().unwrap();
    let writer = std::thread::spawn(move || {
        for _ in 0..18 {
            output.write_all(b"{\"event\":\"ack\"}\n").unwrap();
        }
    });
    let (sender, received) = mpsc::sync_channel(16);
    let result = read_events(input, sender);
    writer.join().unwrap();
    assert!(
        result.is_err(),
        "unsolicited output grew an unbounded queue"
    );
    assert!(received.try_iter().count() <= 16);
}

#[test]
fn held_acknowledgement_uses_the_original_command_deadline() {
    let mut command = Command::new("python3");
    command.args(["-c", "import json,sys; print(json.dumps({'event':'listening','address':'http://127.0.0.1:1/bench/','token':'owned-test'}),flush=True); sys.stdin.read()"]);
    let mut provider =
        ProviderProcess::start_command(command, Instant::now() + Duration::from_secs(3)).unwrap();
    let pid = provider.process.child.id();
    let started = Instant::now();
    let result = provider.ask_until(
        json!({"cmd":"latency","ms":0}),
        "ack",
        started + Duration::from_millis(30),
        Ok,
    );
    let elapsed = started.elapsed();
    provider.finish().unwrap();
    assert!(result.is_err(), "held acknowledgement was accepted");
    assert!(
        elapsed < Duration::from_secs(1),
        "command observation received a fresh deadline"
    );
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "timed-out provider was not reaped"
    );
}

#[test]
fn invalid_startup_reaps_the_owned_child() {
    let pid_file = tempfile::NamedTempFile::new().unwrap();
    let mut command = Command::new("python3");
    command.args(["-c", "import json,os,sys; open(sys.argv[1],'w').write(str(os.getpid())); print(json.dumps({'event':'listening'}),flush=True); sys.stdin.read()"]);
    command.arg(pid_file.path());
    let result = ProviderProcess::start_command(command, Instant::now() + Duration::from_secs(3));
    let pid: u32 = std::fs::read_to_string(pid_file.path())
        .unwrap()
        .parse()
        .unwrap();
    assert!(result.is_err(), "invalid startup was accepted");
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "handshake failure leaked owned child"
    );
}

#[test]
fn unrelated_acknowledgement_is_rejected_and_cleanup_is_joined() {
    let mut command = Command::new("python3");
    command.args(["-c", "import json,sys; print(json.dumps({'event':'listening','address':'http://127.0.0.1:1/bench/','token':'owned-test'}),flush=True); sys.stdin.readline(); print(json.dumps({'event':'ack','cmd':'different'}),flush=True); sys.stdin.read()"]);
    let mut provider =
        ProviderProcess::start_command(command, Instant::now() + Duration::from_secs(3)).unwrap();
    let pid = provider.process.child.id();
    let result = provider.latency(0);
    provider.finish().unwrap();
    assert!(result.is_err(), "unrelated acknowledgement was accepted");
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "protocol error leaked owned child"
    );
}

#[test]
fn timed_out_owner_cannot_accept_a_late_ack_for_another_request() {
    let mut command = Command::new("python3");
    command.args(["-c", "import json,sys; print(json.dumps({'event':'listening','address':'http://127.0.0.1:1/bench/','token':'owned-test'}),flush=True); sys.stdin.readline(); sys.stdin.readline(); print(json.dumps({'event':'ack','cmd':'latency'}),flush=True); sys.stdin.read()"]);
    let mut provider =
        ProviderProcess::start_command(command, Instant::now() + Duration::from_secs(3)).unwrap();
    let pid = provider.process.child.id();
    let first = provider.ask_until(
        json!({"cmd":"latency","ms":0}),
        "ack",
        Instant::now() + Duration::from_millis(30),
        Ok,
    );
    let second = provider.ask_until(
        json!({"cmd":"latency","ms":0}),
        "ack",
        Instant::now() + Duration::from_secs(1),
        Ok,
    );
    provider.finish().unwrap();
    assert!(first.is_err(), "held first acknowledgement was accepted");
    assert!(
        second.is_err(),
        "late acknowledgement was accepted for another request"
    );
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "late-ack control leaked child"
    );
}

#[test]
fn largest_two_source_fixture_command_fits_the_owned_pipe_budget() {
    let mut command = Command::new("python3");
    command.args(["-c", "import json,sys; print(json.dumps({'event':'listening','address':'http://127.0.0.1:1/bench/','token':'owned-test'}),flush=True); line=sys.stdin.readline(); print(json.dumps({'event':'ack','cmd':'sources' if len(line.encode())<=512 else 'oversized'}),flush=True); sys.stdin.read()"]);
    let mut provider =
        ProviderProcess::start_command(command, Instant::now() + Duration::from_secs(3)).unwrap();
    let name = format!("bench-remote-{}", usize::MAX);
    let a = format!("audio/{name}/a");
    let b = format!("audio/{name}/b");
    let result = provider.sources(&name, &[&a, &b]);
    provider.finish().unwrap();
    assert!(
        result.is_ok(),
        "largest actual two-source command exceeded the pipe budget"
    );
}
fn invalid_payload_control(
    reply: Value,
    later_reply: Value,
    probe: impl FnOnce(&mut ProviderProcess) -> (bool, bool),
) {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("later-write");
    let mut command = Command::new("python3");
    command.args(["-c", "import json,sys; print(json.dumps({'event':'listening','address':'http://127.0.0.1:1/bench/','token':'owned-test'}),flush=True); sys.stdin.readline(); print(sys.argv[1],flush=True); line=sys.stdin.readline(); open(sys.argv[2],'w').write('1') if line else None; print(sys.argv[3],flush=True) if line else None; sys.stdin.read()"]);
    command
        .arg(reply.to_string())
        .arg(&marker)
        .arg(later_reply.to_string());
    let mut provider =
        ProviderProcess::start_command(command, Instant::now() + Duration::from_secs(3)).unwrap();
    let (first_rejected, later_rejected) = probe(&mut provider);
    let cleanup = provider.finish();
    let later_write = marker.exists();
    eprintln!(
        "{{\"event\":\"invalid_provider_payload_control\",\"first_rejected\":{first_rejected},\"later_rejected\":{later_rejected},\"later_write\":{later_write}}}"
    );
    assert!(cleanup.is_ok(), "invalid-payload control cleanup failed");
    assert!(first_rejected, "invalid reply payload was accepted");
    assert!(
        later_rejected,
        "invalid reply payload left the provider reusable"
    );
    assert!(!later_write, "invalid reply payload allowed another write");
}

#[test]
fn invalid_stream_count_retires_owner_before_counters_can_write() {
    invalid_payload_control(
        json!({"event":"streams","count":"invalid"}),
        json!({"event":"stats","counters":{"sources":1,"audio":1}}),
        |provider| {
            (
                provider.streams("owned-source").is_err(),
                provider.counters().is_err(),
            )
        },
    );
}

#[test]
fn invalid_counter_retires_owner_before_any_later_command_can_write() {
    invalid_payload_control(
        json!({"event":"stats","counters":{"sources":1,"audio":"invalid"}}),
        json!({"event":"ack","cmd":"latency"}),
        |provider| (provider.counters().is_err(), provider.latency(0).is_err()),
    );
}

#[test]
fn valid_reply_after_validation_deadline_retires_owner_without_later_write() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("later-write");
    let mut command = Command::new("python3");
    command.args(["-c", "import json,sys; print(json.dumps({'event':'listening','address':'http://127.0.0.1:1/bench/','token':'owned-test'}),flush=True); sys.stdin.readline(); print(json.dumps({'event':'ack','cmd':'latency'}),flush=True); line=sys.stdin.readline(); open(sys.argv[1],'w').write('1') if line else None; print(json.dumps({'event':'ack','cmd':'latency'}),flush=True) if line else None; sys.stdin.read()"]);
    command.arg(&marker);
    let mut provider =
        ProviderProcess::start_command(command, Instant::now() + Duration::from_secs(3)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut validator_reached = false;
    let result = provider.ask_until(json!({"cmd":"latency","ms":0}), "ack", deadline, |_| {
        validator_reached = true;
        let gate = (std::sync::Mutex::new(()), std::sync::Condvar::new());
        let mut guard = gate.0.lock().unwrap();
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            guard = gate.1.wait_timeout(guard, remaining).unwrap().0;
        }
        Ok(())
    });
    let later = provider.latency(0);
    let cleanup = provider.finish();
    let later_write = marker.exists();
    eprintln!(
        "{{\"event\":\"acceptance_after_validation_deadline_control\",\"validator_reached\":{validator_reached},\"late_valid_accepted\":{},\"later_write\":{later_write}}}",
        result.is_ok()
    );
    assert!(
        cleanup.is_ok(),
        "validation-deadline control cleanup failed"
    );
    assert!(
        validator_reached,
        "owned ACK did not reach payload validation"
    );
    assert!(
        result.is_err(),
        "valid reply was accepted after validation deadline"
    );
    assert!(later.is_err(), "late validation left provider reusable");
    assert!(!later_write, "late validation allowed another write");
}
