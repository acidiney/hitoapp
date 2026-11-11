import array
import importlib.util
import json
import os
import struct
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('native_buffer_probe', Path(__file__).with_name('native_buffer_probe_analysis.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


def complete():
    return {'event': 'player_complete', 'source_frames': 576000, 'underruns': 0, 'xruns': 0}


def closed():
    return {'event': 'player_closed', 'result': 0}


class NativeBufferQualification(unittest.TestCase):
    def test_completed_public_counter_and_joined_close_qualify(self):
        result = probe.classify_transport(0, [complete(), closed()])
        self.assertTrue(result['transport_success'])

    def test_fault_or_failed_process_cannot_hide_behind_a_full_counter(self):
        for code, events in [
            (17, [complete(), closed()]),
            (0, [complete(), {'event': 'transport_fault', 'stage': 10}, closed()]),
            (0, [complete(), closed(), {'event': 'transport_fault', 'stage': 10}]),
            (0, [complete(), {'event': 'player_closed', 'result': -1}]),
        ]:
            with self.subTest(code=code, events=events):
                self.assertFalse(probe.classify_transport(code, events)['transport_success'])

    def test_a_failed_tail_write_cannot_hide_after_complete_source_and_close(self):
        self.assertFalse(probe.classify_transport(0, [complete(), closed()], [0, 0, -1])['transport_success'])
        self.assertTrue(probe.classify_transport(0, [complete(), closed()], [0, 0])['transport_success'])

    def test_missing_duplicated_short_or_interrupted_output_is_unqualified(self):
        for events in [
            [], [complete()], [closed()], [complete(), complete(), closed()],
            [complete(), closed(), closed()],
            [dict(complete(), source_frames=575999), closed()],
            [dict(complete(), underruns=1), closed()],
            [dict(complete(), xruns=1), closed()],
        ]:
            with self.subTest(events=events):
                self.assertFalse(probe.classify_transport(0, events)['transport_success'])

    def test_stereo_comparison_rejects_a_changed_discarded_channel(self):
        for right, identical in [(1.0, True), (0.0, False), (2.0, False)]:
            stereo = array.array('f', [1.0, right, 3.0, 3.0]).tobytes()
            _, result = probe.mono(stereo)
            self.assertEqual(result['channels_identical'], identical)
            self.assertEqual(result['first_channel_difference_frame'], None if identical else 0)

    def test_only_exact_ordered_counter_with_stereo_and_clean_transport_passes(self):
        for transport, exact, same, expected in [
            (True, True, True, True), (False, True, True, False),
            (True, False, True, False), (True, True, False, False),
        ]:
            with self.subTest(transport=transport, exact=exact, same=same):
                self.assertEqual(probe.classify_fidelity(transport,
                    {'capture': {'exact_full_counter': exact}},
                    {'capture': {'channels_identical': same}}), expected)

    def test_trimming_preserves_internal_silence_and_source_order(self):
        actual, start = probe.active(array.array('f', [0, 0, 1, 0, 2, 0]))
        self.assertEqual(start, 2)
        self.assertEqual(list(actual), [1, 0, 2])


@unittest.skipUnless(os.environ.get('HITO_NATIVE_BUFFER_EVIDENCE'), 'retained native-buffer evidence required')
class RetainedNativeBufferAnalysis(unittest.TestCase):
    def inputs(self, condition):
        root = Path(os.environ['HITO_NATIVE_BUFFER_EVIDENCE']) / condition
        return {
            'accounting': json.loads((root / 'process-accounting.json').read_text()),
            'ledger_bytes': {role: (root / (role + '.observer.bin')).read_bytes()
                             for role in ('writer', 'client', 'sink')},
            'witness_bytes': (root / 'writer-input.f32').read_bytes(),
            'rendered_bytes': (root / 'rendered.f32').read_bytes(),
            'recordings': {role: (root / (role + '.stereo.f32')).read_bytes()
                           for role in ('client', 'sink')},
            'events': [json.loads(line) for line in (root / 'transport-events.jsonl').read_bytes().splitlines()
                       if line.startswith(b'{')],
        }

    def test_recorded_pcm_must_match_complete_export_before_trimming(self):
        for role in ('client', 'sink'):
            for boundary in ('leading', 'trailing'):
                for change in ('missing', 'extra'):
                    with self.subTest(role=role, boundary=boundary, change=change):
                        inputs = self.inputs('candidate30')
                        pcm = inputs['recordings'][role]
                        edge = pcm[:8] if boundary == 'leading' else pcm[-8:]
                        self.assertEqual(edge, bytes(8))
                        if change == 'missing':
                            changed = pcm[8:] if boundary == 'leading' else pcm[:-8]
                        else:
                            changed = bytes(8) + pcm if boundary == 'leading' else pcm + bytes(8)
                        inputs['recordings'][role] = changed
                        with self.assertRaisesRegex(RuntimeError, 'record output/drop accounting mismatch'):
                            probe.analyze(**inputs)

    def test_retained_candidate_passes_and_bad_baseline_fails(self):
        for condition, expected in [('candidate30', True), ('baseline20', False)]:
            with self.subTest(condition=condition):
                result = probe.analyze(**self.inputs(condition))
                self.assertTrue(result['transport_success'])
                self.assertEqual(result['fidelity_pass'], expected)
                self.assertTrue(all(not monitor['ranges'] for monitor in result['monitor_holes'].values()))

    def test_recorder_holes_in_trimmed_silence_fail_exact_stereo_candidate(self):
        for role in ('client', 'sink'):
            for boundary in ('before', 'after'):
                with self.subTest(role=role, boundary=boundary):
                    inputs = self.inputs('candidate30')
                    samples, _ = probe.mono(inputs['recordings'][role])
                    source, leading = probe.active(samples)
                    raw = bytearray(inputs['ledger_bytes'][role])
                    records = probe.read_ledger(raw)['records']
                    matching = []
                    for index, record in enumerate(records):
                        result, start, length, end, pending, hole = record['values'][:6]
                        outside = end <= leading * 8 if boundary == 'before' else start >= (leading + len(source)) * 8
                        if record['kind'] == 6 and result == 0 and pending and length > 0 and not hole and outside:
                            matching.append(index)
                    self.assertTrue(matching, 'retained capture needs a complete silent fragment at this boundary')
                    index = matching[0] if boundary == 'before' else matching[-1]
                    drop = records[index]
                    start, length = drop['values'][1:3]
                    self.assertEqual(inputs['recordings'][role][start:start + length], bytes(length))
                    peek_indexes = []
                    for previous in range(index - 1, -1, -1):
                        peek = records[previous]
                        if peek['kind'] == 6:
                            break
                        if peek['kind'] == 5 and peek['values'][:4] == (0, length, 0, start):
                            peek_indexes.append(previous)
                    self.assertTrue(peek_indexes, 'hole drop must retain matching successful peeks')
                    for peek_index in peek_indexes:
                        struct.pack_into('<q', raw, 80 + peek_index * 168 + 40 + 2 * 8, 1)
                    struct.pack_into('<q', raw, 80 + index * 168 + 40 + 5 * 8, 1)
                    inputs['ledger_bytes'][role] = bytes(raw)
                    result = probe.analyze(**inputs)
                    self.assertEqual(result['monitor_holes'][role]['ranges'],
                                     [{'start_frame': start // 8, 'frames': length // 8}])
                    self.assertTrue(result['transport_success'])
                    self.assertTrue(all(stats['exact_full_counter'] for stats in result['stats'].values()))
                    self.assertTrue(all(stereo['channels_identical'] for stereo in result['stereo_channel_checks'].values()))
                    self.assertFalse(result['fidelity_pass'])


if __name__ == '__main__':
    unittest.main()
