import array
import hashlib
import sys
import struct


def read_ledger(raw):
    if raw[:8] != b'HITOPULS' or raw[-8:] != b'EXITDONE':
        raise RuntimeError('observer footer missing')
    header = struct.unpack('<9Q', raw[8:80])
    if header[0] != 1 or header[4] or header[5] or len(raw) != 88 + header[3] * 168:
        raise RuntimeError('observer invalid, incomplete or overflowed')
    records = []
    for offset in range(80, len(raw) - 8, 168):
        prefix = struct.unpack('<5Q', raw[offset:offset + 40])
        values = struct.unpack('<16q', raw[offset + 40:offset + 168])
        if prefix[0] != prefix[1] or prefix[0] != len(records) + 1:
            raise RuntimeError('observer record not committed')
        records.append({'sequence': prefix[0], 'kind': prefix[2], 'stream': prefix[3],
                        'observed_monotonic_ns': prefix[4], 'values': values})
    return {'header': header, 'records': records}


def mono(stereo):
    samples = array.array('f')
    samples.frombytes(stereo)
    if sys.byteorder != 'little':
        samples.byteswap()
    left = array.array('f', samples[::2])
    right = array.array('f', samples[1::2])
    first_channel_difference = next((index for index, (a, b) in enumerate(zip(left, right)) if a != b), None)
    return left, {'channels_identical': left.tobytes() == right.tobytes(),
        'first_channel_difference_frame': first_channel_difference}


def active(samples):
    start = next((index for index, value in enumerate(samples) if value), None)
    if start is None:
        return array.array('f'), 0
    end = next(index for index in range(len(samples) - 1, -1, -1) if samples[index])
    return samples[start:end + 1], start


def classify_transport(writer_exit, events, write_results=()):
    faults = [row for row in events if row['event'] == 'transport_fault']
    completions = [row for row in events if row['event'] == 'player_complete']
    closes = [row for row in events if row['event'] == 'player_closed']
    exact = (len(completions) == 1 and completions[0].get('source_frames') == 576000
        and completions[0].get('underruns') == 0 and completions[0].get('xruns') == 0)
    clean = len(closes) == 1 and closes[0].get('result') == 0
    failed_writes = sum(result < 0 for result in write_results)
    return {'faults':faults,'exact_completion':exact,'clean_close':clean, 'failed_writes':failed_writes,
        'transport_success':writer_exit == 0 and not faults and not failed_writes and exact and clean}


def classify_fidelity(transport_success, stats, stereo):
    return (transport_success and all(value['exact_full_counter'] for value in stats.values())
        and all(value['channels_identical'] for value in stereo.values()))


def analyze(accounting, ledger_bytes, witness_bytes, rendered_bytes, recordings, events):
    for role in ('writer', 'client', 'sink'):
        report = accounting[role]
        if not report['joined'] or report['eof'] != ['stderr', 'stdout'] or report['read_errno'] or report['overflow'] or report['stderr_overflow']:
            raise RuntimeError('owned process accounting incomplete')
    if accounting['sink']['exit_code'] != 0 or accounting['sink']['stderr_reason'] != 'empty':
        raise RuntimeError('sink close status unqualified')
    if accounting['writer']['exit_code'] != 0 or accounting['writer']['stderr_reason'] != 'empty':
        raise RuntimeError('writer close status unqualified')
    if accounting['client']['exit_code'] != 1 or accounting['client']['stderr_reason'] != 'stream_entity_killed':
        raise RuntimeError('client close reason unqualified')
    if any(accounting['client']['eof_observed_monotonic_ns'][name] < accounting['close_started_observed_monotonic_ns'] for name in ('stdout', 'stderr')):
        raise RuntimeError('client ended before close marker')
    ledgers = {role: read_ledger(ledger_bytes[role]) for role in ('writer', 'client', 'sink')}
    for role, ledger in ledgers.items():
        if ledger['header'][1] != {'writer': 1, 'client': 2, 'sink': 3}[role] or ledger['header'][8] != 1:
            raise RuntimeError('observer role or monotonic clock label mismatch')
        if ledger['header'][2] != accounting[role]['pid']:
            raise RuntimeError('observer process identity mismatch')
    writes = [row for row in ledgers['writer']['records'] if row['kind'] == 3]
    held = [row for row in writes if row['values'][7]]
    holds = [row for row in ledgers['writer']['records'] if row['kind'] == 10]
    arms = [row for row in ledgers['writer']['records'] if row['kind'] == 9]
    stimulus = {'armed': len(arms), 'holds': len(holds), 'held_writes': len(held),
        'source_frame': arms[0]['values'][0] if arms else None,
        'completion_gap_ns': held[0]['values'][8] if held else None,
        'clock': 'CLOCK_MONOTONIC completion observations, not CPAL timestamp'}
    if len(held) != 1 or len(holds) != 1 or len(arms) != 1 or stimulus['source_frame'] != 230400 or not (40000000 <= stimulus['completion_gap_ns'] <= 45000000) or not holds[0]['values'][5]:
        raise RuntimeError('single late-write stimulus unqualified')
    underflows = [row for row in ledgers['writer']['records'] if row['kind'] == 4]
    holes = {}
    for role in ('client', 'sink'):
        cursor = 0
        ranges = []
        pending = None
        peek_count = 0
        successful_drops = 0
        for row in ledgers[role]['records']:
            if row['kind'] == 5:
                result, length, hole, observed_cursor = row['values'][:4]
                if observed_cursor != cursor:
                    raise RuntimeError('peek changed record cursor')
                if result == 0 and length > 0:
                    peek_count += 1
                    current = (length, hole)
                    if pending is not None and current != pending:
                        raise RuntimeError('repeated peek changed pending fragment')
                    pending = current
                continue
            if row['kind'] != 6:
                continue
            result, before, length, after, had_pending, hole = row['values'][:6]
            if before != cursor:
                raise RuntimeError('record cursor discontinuity')
            if before % 8 or length % 8:
                raise RuntimeError('record fragment not frame aligned')
            if result == 0 and had_pending:
                if pending != (length, hole):
                    raise RuntimeError('drop did not match last successful peek')
                if after != before + length:
                    raise RuntimeError('successful drop cursor invalid')
                if hole:
                    ranges.append({'start_frame': before // 8, 'frames': length // 8})
                cursor = after
                pending = None
                successful_drops += 1
            elif after != before:
                raise RuntimeError('failed drop advanced cursor')
        if pending is not None:
            raise RuntimeError('record process exited with an undropped fragment')
        exported_bytes = len(recordings[role])
        if accounting[role]['stdout_bytes'] != cursor or exported_bytes != cursor or exported_bytes % 8:
            raise RuntimeError('record output/drop accounting mismatch')
        holes[role] = {'ranges': ranges, 'dropped_bytes': cursor,
                       'exported_bytes': exported_bytes,
                       'unexported_tail_bytes': cursor - exported_bytes,
                       'peek_events': peek_count, 'successful_drops': successful_drops}
    witness = array.array('f')
    if len(witness_bytes) % 4:
        raise RuntimeError('writer witness byte alignment invalid')
    witness.frombytes(witness_bytes)
    if len(witness) != ledgers['writer']['header'][6]:
        raise RuntimeError('writer witness length disagrees with observer')
    witness_hash = hashlib.sha256(witness_bytes).hexdigest()
    begins = [row for row in ledgers['writer']['records'] if row['kind'] == 2]
    if len(begins) != len(writes):
        raise RuntimeError('write begin/end count mismatch')
    submitted_frames = 0
    accepted_total_bytes = 0
    for begin, end in zip(begins, writes):
        b, e = begin['values'], end['values']
        if b[0] != b[5] * 8 or b[1] != 0 or b[2] != 0 or b[4] != submitted_frames:
            raise RuntimeError('write arguments or PCM witness offsets invalid')
        if end['stream'] != begin['stream'] or e[0] != b[0] or e[4:6] != b[4:6] or e[2] != accepted_total_bytes:
            raise RuntimeError('write outcome does not match submitted arguments')
        submitted_frames += b[5]
        if e[1] >= 0:
            accepted_total_bytes += e[0]
        if e[3] != accepted_total_bytes:
            raise RuntimeError('accepted write byte accounting mismatch')
    if submitted_frames != len(witness):
        raise RuntimeError('writer PCM witness incomplete')
    accepted = array.array('f')
    for write in writes:
        values = write['values']
        if values[1] >= 0:
            accepted.extend(witness[values[4]:values[4] + values[5]])
    expected = array.array('f', (((index % 288000) % 4093 * 8 + 8) / 32768 for index in range(576000)))
    client_samples, client_stereo = mono(recordings['client'])
    sink_samples, sink_stereo = mono(recordings['sink'])
    stereo = {'client':client_stereo,'sink':sink_stereo}
    stats = {}
    for role, samples in [('accepted', accepted), ('client', client_samples), ('sink', sink_samples)]:
        trimmed, leading = active(samples)
        heard = array.array('f', (value for value in trimmed if value))
        mismatch = next((index for index, (a, b) in enumerate(zip(trimmed, expected)) if a != b), None)
        order = next((index for index, (a, b) in enumerate(zip(heard, expected)) if a != b), None)
        stats[role] = {'frames': len(samples), 'leading_zero_frames': leading,
            'active_frames': len(trimmed), 'nonzero_frames': len(heard),
            'internal_zero_frames': len(trimmed) - len(heard), 'first_expected_mismatch': mismatch,
            'first_nonzero_order_mismatch': order,
            'active_sha256': hashlib.sha256(trimmed.tobytes()).hexdigest(),
            'exact_full_counter': trimmed.tobytes() == expected.tobytes(),
            'prefix_before_stimulus_exact': trimmed[:230400].tobytes() == expected[:230400].tobytes()}
    rendered = array.array('f')
    rendered.frombytes(rendered_bytes)
    rendered_active, _ = active(rendered)
    if rendered_active.tobytes() != expected.tobytes():
        raise RuntimeError('rendered public counter incomplete')
    outcome = classify_transport(accounting['writer']['exit_code'], events, (row['values'][1] for row in writes))
    faults = outcome['faults']
    exact_completion = outcome['exact_completion']
    clean_close = outcome['clean_close']
    transport_success = outcome['transport_success']
    fidelity_pass = (all(not monitor['ranges'] for monitor in holes.values())
        and classify_fidelity(transport_success, stats, stereo))
    result = {'stimulus': stimulus, 'stats': stats, 'stereo_channel_checks':stereo,'underflows': underflows, 'monitor_holes': holes,
        'actual_attributes': [row for row in ledgers['writer']['records'] if row['kind'] == 7],
        'writer_fault_events': faults, 'failed_observed_writes':outcome['failed_writes'], 'transport_success': transport_success,
        'exact_counter_complete': exact_completion, 'clean_close': clean_close,
        'fidelity_pass': fidelity_pass,
        'writer_pcm_accounting': {'bytes':len(witness_bytes),'frames':len(witness),'sha256':witness_hash,
            'submitted_frames':submitted_frames,'accepted_total_bytes':accepted_total_bytes},
        'expected_counter_sha256':hashlib.sha256(expected.tobytes()).hexdigest(),
        'ledgers': ledgers, 'public_player': 'STOCK_CPAL_NO_PRODUCT_FAULT_HOOK',
        'historical189_and_physical_output': 'OPEN_NOT_CLAIMED'}
    return result
