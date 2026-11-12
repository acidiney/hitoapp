import hashlib
import json
import os
from pathlib import Path
from native_buffer_probe_analysis import analyze
import select
import signal
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
BINARY = None
OBSERVER = None
EVIDENCE = None
MAX_AUDIO = 30 * 48000 * 8
DEADLINE = time.monotonic() + 45
RUN_DEADLINE = DEADLINE - 8
OWNED = []
RESULTS = {}


class Child:
    def __init__(self, command, role=None, input_pipe=False):
        environment = os.environ.copy()
        if role:
            environment.update(LD_PRELOAD=str(OBSERVER),
                               HITO_OBSERVER_ROLE=role, HITO_OBSERVER_DIR=str(EVIDENCE),
                               HITO_NATIVE_BUFFER_OWNER_EXE=str(Path(shutil.which(command[0]) or command[0]).resolve()))
        else:
            environment.pop('LD_PRELOAD', None)
            environment.pop('HITO_OBSERVER_ROLE', None)
            environment.pop('HITO_OBSERVER_DIR', None)
        self.process = subprocess.Popen(command, env=environment,
            stdin=subprocess.PIPE if input_pipe else subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if role == 'writer':
            environment_probe = environment.get('HITO_NATIVE_BUFFER_PROBE')
            if environment_probe != '1':
                raise RuntimeError('explicit public probe activation missing')
        self.role = role
        self.capacity = MAX_AUDIO if role in ('client', 'sink') else 65536
        self.output = bytearray(self.capacity)
        self.length = 0
        self.stderr = bytearray(512)
        self.error_bytes = 0
        self.error_used = 0
        self.eof = set()
        self.eof_ns = {}
        self.read_errno = None
        self.overflow = False
        self.joined = False
        self.closed = False
        for pipe in (self.process.stdout, self.process.stderr):
            os.set_blocking(pipe.fileno(), False)
        if input_pipe:
            os.set_blocking(self.process.stdin.fileno(), False)
        OWNED.append(self)

    def pump(self):
        if self.closed:
            return
        for name in ('stdout', 'stderr'):
            if name in self.eof:
                continue
            pipe = getattr(self.process, name)
            while True:
                try:
                    data = os.read(pipe.fileno(), 8192)
                except BlockingIOError:
                    break
                except InterruptedError:
                    continue
                except OSError as error:
                    self.read_errno = error.errno
                    self.eof.add(name)
                    break
                if not data:
                    self.eof.add(name)
                    self.eof_ns[name] = time.monotonic_ns()
                    break
                if name == 'stdout':
                    keep = min(len(data), self.capacity - self.length)
                    self.output[self.length:self.length + keep] = data[:keep]
                    self.length += keep
                    self.overflow |= keep != len(data)
                else:
                    self.error_bytes += len(data)
                    keep = min(len(data), 512 - self.error_used)
                    self.stderr[self.error_used:self.error_used + keep] = data[:keep]
                    self.error_used += keep
        self.process.poll()

    def bytes(self):
        return bytes(memoryview(self.output)[:self.length])

    def events(self):
        return [json.loads(line) for line in self.bytes().split(b'\n')[:-1] if line.startswith(b'{')]

    def reason(self):
        retained = bytes(memoryview(self.stderr)[:self.error_used])
        if not self.error_bytes:
            return 'empty'
        if self.error_bytes == 28 and retained == b'Stream error: Entity killed\n':
            return 'stream_entity_killed'
        return 'unknown'

    def finish(self, timeout=3):
        until = min(DEADLINE, time.monotonic() + timeout)
        while self.process.poll() is None or len(self.eof) != 2:
            pump_all()
            if self.process.poll() is not None and len(self.eof) == 2:
                break
            if time.monotonic() >= until:
                raise RuntimeError('owned child did not exit and drain')
            select.select([], [], [], 0.001)
        self.pump()
        self.process.wait(timeout=0.1)
        self.joined = True
        self.process.stdout.close()
        self.process.stderr.close()
        if self.process.stdin and not self.process.stdin.closed:
            self.process.stdin.close()
        self.closed = True
        return self.metadata()

    def metadata(self):
        retained = bytes(memoryview(self.stderr)[:self.error_used])
        return {'pid': self.process.pid, 'role': self.role, 'exit_code': self.process.returncode,
            'stdout_bytes': self.length, 'trailing_frame_bytes': self.length % 8 if self.role in ('client', 'sink') else 0,
            'stderr_bytes': self.error_bytes, 'stderr_reason': self.reason(),
            'stderr_overflow': self.error_bytes > 512, 'stderr_sha256': hashlib.sha256(retained).hexdigest(),
            'read_errno': self.read_errno, 'overflow': self.overflow, 'eof': sorted(self.eof),
            'eof_observed_monotonic_ns': self.eof_ns, 'joined': self.joined}


def pump_all():
    for child in OWNED:
        child.pump()


def wait(predicate, timeout=5):
    until = min(RUN_DEADLINE, time.monotonic() + timeout)
    while True:
        pump_all()
        result = predicate()
        if result is not None and result is not False:
            return result
        if time.monotonic() >= until:
            raise RuntimeError('owned transport condition timed out')
        select.select([], [], [], 0.001)


def pulse(kind):
    child = Child(['pactl', '--format=json', 'list', kind])
    report = child.finish(timeout=2)
    if report['exit_code'] != 0 or report['overflow'] or report['stderr_bytes']:
        raise RuntimeError('private Pulse inspection failed')
    data = json.loads(child.bytes())
    OWNED.remove(child)
    return data


def verify_source(writer):
    inputs = pulse('sink-inputs')
    matched = [item for item in inputs if item['properties'].get('application.process.id') == str(writer.process.pid)]
    if not matched:
        return None
    if len(inputs) != 1 or len(matched) != 1:
        raise RuntimeError('private playback client is ambiguous')
    item = matched[0]
    if item['properties'].get('application.process.user') != 'hito-desktop':
        raise RuntimeError('private playback UID mismatch')
    uid = Path(f'/proc/{writer.process.pid}/status').read_text().split('Uid:\t', 1)[1].split()[0]
    if int(uid) != os.getuid() or os.getuid() != 10001:
        raise RuntimeError('owned process UID mismatch')
    sinks = pulse('sinks')
    if len(sinks) != 1 or sinks[0]['name'] != 'hito-test' or item['sink'] != sinks[0]['index']:
        raise RuntimeError('private sink mismatch')
    return item['index']


def capture(role, index=None):
    command = ['parec', '--device=hito-test.monitor', '--format=float32le', '--rate=48000',
               '--channels=2', '--latency-msec=20']
    if index is not None:
        command.append(f'--monitor-stream={index}')
    return Child(command, role=role)


def setup_and_run():
    server = os.environ.get('PULSE_SERVER', '')
    if not server.startswith('unix:/tmp/hito-audio-') or not server.endswith('/pulse.sock'):
        raise RuntimeError('private backend identity unavailable')
    sink = capture('sink')
    wait(lambda: sink.length >= 4800 * 8)
    writer = Child([str(BINARY), '--ignored', '--exact', 'a_late_backend_write_preserves_every_source_frame', '--test-threads=1', '--nocapture'], role='writer', input_pipe=True)
    index = wait(lambda: verify_source(writer))
    client = capture('client', index)
    wait(lambda: client.length >= 4800 * 8)
    if writer.process.poll() is not None or client.process.poll() is not None:
        raise RuntimeError('owned playback or capture ended before readiness')
    if any(client.bytes()) or any(sink.bytes()):
        raise RuntimeError('counter started before capture readiness')
    if verify_source(writer) != index:
        raise RuntimeError('owned input changed before counter')
    wait(lambda: any(item['event'] == 'player_ready' for item in writer.events()))
    configurations = [item for item in writer.events() if item['event'] == 'transport_configured']
    if len(configurations) != 1:
        raise RuntimeError('actual configuration ambiguous')
    measured_period = configurations[0]['period_frames']
    if measured_period <= 0 or 230400 % measured_period:
        raise RuntimeError('shared stimulus point is not a full-callback boundary')
    RESULTS['actual_configuration'] = configurations[0]
    RESULTS['readiness'] = {'source_frame':230400,'alignment_verified_before_counter':True,
        'sink_input': index, 'client_frames': client.length // 8,
        'sink_frames': sink.length // 8, 'counter_not_started': True,
        'ready_observed_monotonic_ns': time.monotonic_ns()}
    os.write(writer.process.stdin.fileno(), b'S')
    wait(lambda: any(item['event'] in ('player_complete', 'transport_fault') for item in writer.events()), timeout=25)
    mark = sink.length
    wait(lambda: sink.length >= mark + 4800 * 8)
    RESULTS['close_started_observed_monotonic_ns'] = time.monotonic_ns()
    os.write(writer.process.stdin.fileno(), b'C')
    RESULTS['writer'] = writer.finish(timeout=3)
    RESULTS['client'] = client.finish(timeout=3)
    if any(item['index'] == index for item in pulse('sink-inputs')):
        raise RuntimeError('owned playback input remained after close')
    RESULTS['input_gone_after_close'] = True
    RESULTS['pulse_healthy_after_close'] = len(pulse('sinks')) == 1
    mark = sink.length
    wait(lambda: sink.length >= mark + 14400 * 8)
    if sink.process.poll() is None:
        sink.process.terminate()
    RESULTS['sink'] = sink.finish(timeout=3)
    return writer, client, sink


def export_children():
    for child in OWNED:
        if child.role in ('client', 'sink'):
            (EVIDENCE / f'{child.role}.stereo.f32').write_bytes(child.bytes())
        elif child.role == 'writer':
            (EVIDENCE / 'transport-events.jsonl').write_bytes(child.bytes())
    (EVIDENCE / 'process-accounting.json').write_text(json.dumps(RESULTS, indent=2) + '\n')


def qualify(writer, client, sink):
    result = analyze(RESULTS,
        {role: (EVIDENCE / f'{role}.observer.bin').read_bytes() for role in ('writer', 'client', 'sink')},
        (EVIDENCE / 'writer-input.f32').read_bytes(), (EVIDENCE / 'rendered.f32').read_bytes(),
        {'client': client.bytes(), 'sink': sink.bytes()}, writer.events())
    (EVIDENCE / 'stimulus-qualification.json').write_text(json.dumps(result['stimulus'], indent=2) + '\n')
    (EVIDENCE / 'transport-analysis.json').write_text(json.dumps(result, indent=2) + '\n')
    (EVIDENCE / 'qualification.json').write_text(json.dumps({'status':'BOUNDARIES_AND_STIMULUS_COMPLETE',
        'transport_success':result['transport_success'], 'fidelity_pass':result['fidelity_pass']}) + '\n')
    if not result['fidelity_pass']:
        raise AssertionError('strict counter fidelity or transport-success assertion failed')


def main():
    global BINARY, OBSERVER, EVIDENCE
    if len(sys.argv) != 4:
        return 4
    BINARY, OBSERVER, EVIDENCE = (Path(argument).resolve() for argument in sys.argv[1:])
    if not BINARY.is_file() or not OBSERVER.is_file() or not EVIDENCE.is_dir() or any(EVIDENCE.iterdir()):
        return 4
    os.environ['HITO_NATIVE_BUFFER_PROBE'] = '1'
    os.environ['HITO_CAPTURE_DUMP'] = str(EVIDENCE)
    def interrupted(number, frame):
        del number, frame
        raise InterruptedError('owned experiment interrupted')
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    (EVIDENCE / 'qualification.json').write_text('{"status":"UNVALIDATED"}\n')
    outcome = None
    failed = None
    try:
        outcome = setup_and_run()
    except BaseException as error:
        failed = type(error).__name__
    finally:
        for child in reversed(OWNED):
            if not child.closed:
                if child.process.poll() is None:
                    child.process.terminate()
                try:
                    child.finish(timeout=1)
                except BaseException:
                    if child.process.poll() is None:
                        child.process.kill()
                    try:
                        child.finish(timeout=1)
                    except BaseException:
                        RESULTS['cleanup_incomplete'] = True
            if child.role:
                RESULTS[child.role] = child.metadata()
        export_children()
    if failed:
        (EVIDENCE / 'failure.json').write_text(json.dumps({'error_kind': failed}) + '\n')
        return 2
    try:
        qualify(*outcome)
    except BaseException as error:
        (EVIDENCE / 'failure.json').write_text(json.dumps({'error_kind': type(error).__name__}) + '\n')
        return 3
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
