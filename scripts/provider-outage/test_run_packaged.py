import hashlib
from contextlib import redirect_stdout
import importlib.util
from io import StringIO
import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

RUNNER_PATH = Path(__file__).with_name("run-packaged.py")
SPEC = importlib.util.spec_from_file_location("provider_outage_runner", RUNNER_PATH)
RUNNER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RUNNER
SPEC.loader.exec_module(RUNNER)

ROOT = RUNNER.ROOT


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    return path


class PackagedInputTests(unittest.TestCase):
    def test_current_embedded_migrations_are_exactly_1_through_98(self):
        inventory = RUNNER.migration_inventory(ROOT / "crates/core/migrations")
        self.assertEqual(inventory.versions, tuple(range(1, 99)))
        self.assertEqual(inventory.latest, 98)
        self.assertEqual(inventory.after(54), 44)
        self.assertTrue(RUNNER.current_migration_inventory(inventory))
        self.assertTrue(RUNNER.migration_receipt_matches({
            "event": "database_migration_finished",
            "outcome": "complete",
            "previous_version": 54,
            "current_version": 98,
            "applied_count": 44,
        }, inventory))

    def test_noncontiguous_inventory_uses_versions_not_file_count(self):
        with tempfile.TemporaryDirectory() as temporary:
            migrations = Path(temporary)
            for name in (
                "0001_identity.sql", "0054_manual_sources.sql",
                "0097_credit_member_choices.sql", "0098_source_resolution_failure.sql",
            ):
                (migrations / name).write_text("SELECT 1;\n")
            inventory = RUNNER.migration_inventory(migrations)
        self.assertEqual(inventory.versions, (1, 54, 97, 98))
        self.assertEqual(inventory.latest, 98)
        self.assertEqual(inventory.after(54), 2)
        self.assertEqual(tuple(v for v in inventory.versions if v > 54), (97, 98))
        self.assertFalse(RUNNER.current_migration_inventory(inventory))

    def test_invalid_receipts_do_not_match_the_current_embedded_list(self):
        inventory = RUNNER.migration_inventory(ROOT / "crates/core/migrations")
        valid = {
            "event": "database_migration_finished",
            "outcome": "complete",
            "previous_version": 54,
            "current_version": 98,
            "applied_count": 44,
        }
        for field, value in (
            ("previous_version", 53), ("current_version", 97),
            ("applied_count", 43), ("applied_count", True),
        ):
            with self.subTest(field=field, value=value):
                self.assertFalse(RUNNER.migration_receipt_matches(dict(valid, **{field: value}), inventory))
        self.assertFalse(RUNNER.migration_receipt_matches({"event": "database_migration_finished"}, inventory))

    def test_captured_inventory_rejects_forged_summary_and_omitted_migration(self):
        inventory = RUNNER.migration_inventory(ROOT / "crates/core/migrations")
        receipt = {
            "current_version": 98,
            "total_migrations": 98,
            "migrations_after_legacy": 44,
            "entries": [
                {"version": version, "name": name, "sha256": digest}
                for version, name, digest in inventory.entries
            ],
        }
        self.assertEqual(RUNNER.migration_inventory_from_receipt(receipt), inventory)
        for change in (
            {"current_version": 97}, {"total_migrations": 97},
            {"migrations_after_legacy": 43},
        ):
            with self.subTest(change=change):
                with self.assertRaises(ValueError):
                    RUNNER.migration_inventory_from_receipt(dict(receipt, **change))
        with self.assertRaises(ValueError):
            RUNNER.migration_inventory_from_receipt(dict(receipt, entries=receipt["entries"][:-1]))

    def test_source_receipt_bound_accepts_one_mib_and_rejects_oversize(self):
        with tempfile.TemporaryDirectory() as temporary:
            receipt = Path(temporary) / "source.json"
            payload = b'{"source_head":"owned"}'
            receipt.write_bytes(payload + b" " * (RUNNER.SOURCE_RECEIPT_MAX_BYTES - len(payload)))
            self.assertEqual(RUNNER.bounded_json(receipt, RUNNER.SOURCE_RECEIPT_MAX_BYTES), {"source_head": "owned"})
            receipt.write_bytes(payload + b" " * RUNNER.SOURCE_RECEIPT_MAX_BYTES)
            with self.assertRaises(ValueError):
                RUNNER.bounded_json(receipt, RUNNER.SOURCE_RECEIPT_MAX_BYTES)

    def test_legacy_source_routes_to_its_root_dockerfile_contract(self):
        with tempfile.TemporaryDirectory() as temporary:
            tree = Path(temporary)
            helper = write(tree / "scripts/artwork/source-pin.py", "legacy helper\n")
            dockerfile = write(tree / "Dockerfile", "legacy runtime\n")
            with (
                patch.object(RUNNER, "LEGACY_SOURCE_PIN_SHA256", hashlib.sha256(helper.read_bytes()).hexdigest()),
                patch.object(RUNNER, "LEGACY_ROOT_DOCKERFILE_SHA256", hashlib.sha256(dockerfile.read_bytes()).hexdigest()),
            ):
                contract = RUNNER.source_pin_contract("HITO_OUTAGE_LEGACY", tree)
                self.assertEqual(contract.schema, "legacy-root-dockerfile-v1")
                self.assertEqual(contract.helper, helper)
                self.assertEqual(contract.dockerfile, "Dockerfile")
                write(tree / "Dockerfile", "changed runtime\n")
                with self.assertRaises(ValueError):
                    RUNNER.source_pin_contract("HITO_OUTAGE_LEGACY", tree)

    def test_current_source_routes_to_relocated_server_dockerfile_contract(self):
        with tempfile.TemporaryDirectory() as temporary:
            tree = Path(temporary)
            helper = write(tree / "scripts/artwork/source-pin.py", "current helper\n")
            write(tree / "docker/server.Dockerfile", "current runtime\n")
            with patch.object(RUNNER, "CURRENT_SOURCE_PIN_SHA256", hashlib.sha256(helper.read_bytes()).hexdigest()):
                contract = RUNNER.source_pin_contract("HITO_PACKAGED", tree)
                self.assertEqual(contract.schema, "current-server-dockerfile-v1")
                self.assertEqual(contract.helper, helper)
                self.assertEqual(contract.dockerfile, "docker/server.Dockerfile")
                write(tree / "scripts/artwork/source-pin.py", "changed helper\n")
                with self.assertRaises(ValueError):
                    RUNNER.source_pin_contract("HITO_PACKAGED", tree)

    def test_selected_module_volume_is_required_and_propagated_to_compose(self):
        name = "hito-cred-e2e_node-modules"
        selected = getattr(RUNNER, "selected_modules_volume", lambda _environment: None)
        self.assertEqual(selected({"HITO_OUTAGE_MODULES_VOLUME": name}), name)
        matches = getattr(RUNNER, "module_volume_matches", lambda _name, _volume: False)
        valid = {
            "Name": name,
            "CreatedAt": "2026-11-13T02:35:42Z",
            "Driver": "local",
            "Labels": {
                "com.docker.compose.project": "hito-cred-e2e",
                "com.docker.compose.volume": "node-modules",
            },
        }
        self.assertTrue(matches(name, valid))
        self.assertFalse(matches("hito_node-modules", valid))
        self.assertFalse(matches(name, dict(valid, Labels={})))
        with tempfile.TemporaryDirectory() as temporary:
            with patch.dict("os.environ", {"HITO_OUTAGE_MODULES_VOLUME": name}):
                with patch.object(RUNNER.tempfile, "mkdtemp", return_value=temporary):
                    gate = RUNNER.Gate()
                with patch.object(RUNNER, "command", return_value=b"") as run:
                    gate.compose_run("config")
                self.assertEqual(
                    run.call_args.kwargs.get("env", {}).get("HITO_OUTAGE_MODULES_VOLUME"),
                    name,
                )
            with self.assertRaises(ValueError):
                selected({})

    def test_failing_module_command_reports_safe_phase_and_exit(self):
        sentinel = "private-command-output-sentinel"
        class FailedModuleGate:
            phase = "module_pin_before"
            evidence = "/owned/evidence"

            def execute(self):
                RUNNER.command([
                    sys.executable, "-c", f"import sys; print({sentinel!r}); sys.exit(17)",
                ])

            def cleanup(self):
                return None

        output = StringIO()
        handlers = {number: signal.getsignal(number) for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)}
        with (
            patch.object(RUNNER, "Gate", FailedModuleGate),
            patch.object(RUNNER.signal, "signal"),
            redirect_stdout(output),
        ):
            status = RUNNER.main()
        self.assertEqual(
            {number: signal.getsignal(number) for number in handlers},
            handlers,
        )
        self.assertEqual(status, 1)
        result = json.loads(output.getvalue())
        self.assertEqual(
            result.get("failure"),
            {"phase": "module_pin_before", "kind": "command_exit", "exit_code": 17},
        )
        self.assertNotIn(sentinel, output.getvalue())


    def test_eof_before_owned_process_exit_reports_the_original_deadline(self):
        class Selector:
            def __init__(self):
                self.fileobj = None

            def register(self, fileobj, _events):
                self.fileobj = fileobj

            def get_map(self):
                return {1: self.fileobj} if self.fileobj is not None else {}

            def select(self, _timeout):
                from types import SimpleNamespace
                return [(SimpleNamespace(fd=7, fileobj=self.fileobj), None)]

            def unregister(self, _fileobj):
                self.fileobj = None

            def close(self):
                return None

        process = unittest.mock.Mock()
        process.pid = 123
        process.stdout = unittest.mock.Mock()
        process.poll.return_value = None
        process.wait.side_effect = [
            subprocess.TimeoutExpired("owned", 0.1),
            0,
        ]
        with (
            patch.object(RUNNER.subprocess, "Popen", return_value=process),
            patch.object(RUNNER.selectors, "DefaultSelector", Selector),
            patch.object(RUNNER.os, "read", return_value=b""),
            patch.object(RUNNER.os, "killpg") as kill_group,
            patch.object(RUNNER.time, "monotonic", side_effect=[1.0, 1.0, 1.0, 32.0]),
        ):
            with self.assertRaises(RUNNER.OwnedCommandFailure) as caught:
                RUNNER.command(["owned"], seconds=30, phase="module_pin_before")
        self.assertEqual(
            RUNNER.safe_failure("fallback", caught.exception),
            {"phase": "module_pin_before", "kind": "deadline"},
        )
        kill_group.assert_called_once_with(123, signal.SIGKILL)
        process.stdout.close.assert_called_once()

    def test_cleanup_keeps_first_safe_command_failure_and_continues_verification(self):
        with tempfile.TemporaryDirectory() as temporary:
            gate = object.__new__(RUNNER.Gate)
            gate.phase = "cleanup"
            gate.browser = None
            gate.modules_before = {"version": "1.56.1"}
            gate.modules_volume_identity = None
            gate.project = "owned-project"
            gate.run_id = "owned-run"
            gate.container_services = {}
            gate.evidence = Path(temporary)
            gate.browser_output = bytearray()
            gate.module_pin = unittest.mock.Mock(
                side_effect=RUNNER.OwnedCommandFailure("module_pin_after", "deadline")
            )
            calls = []

            def command_result(arguments, **_kwargs):
                calls.append(tuple(arguments))
                return b""

            with patch.object(RUNNER, "command", side_effect=command_result):
                with self.assertRaises(ValueError) as caught:
                    gate.cleanup()

            self.assertEqual(
                RUNNER.safe_failure("fallback", caught.exception),
                {"phase": "module_pin_after", "kind": "deadline"},
            )
            cleanup = json.loads((gate.evidence / "cleanup.json").read_text())
            self.assertEqual(cleanup["failures"], ["module_pin"])
            self.assertTrue(any(call[:3] == ("docker", "ps", "-aq") for call in calls))
            self.assertTrue(any(call[:4] == ("docker", "volume", "ls", "-q") for call in calls))

    def test_main_preserves_execution_failure_and_reports_cleanup_failure(self):
        class FailedGate:
            phase = "legacy_start"
            evidence = "/owned/evidence"

            def execute(self):
                raise RUNNER.OwnedCommandFailure("legacy_start", "command_exit", 17)

            def cleanup(self):
                raise RUNNER.OwnedCommandFailure("module_pin_after", "deadline")

        output = StringIO()
        with (
            patch.object(RUNNER, "Gate", FailedGate),
            patch.object(RUNNER.signal, "signal"),
            redirect_stdout(output),
        ):
            status = RUNNER.main()
        result = json.loads(output.getvalue())
        self.assertEqual(status, 1)
        self.assertEqual(
            result.get("failure"),
            {"phase": "legacy_start", "kind": "command_exit", "exit_code": 17},
        )
        self.assertEqual(
            result.get("cleanup_failure"),
            {"phase": "module_pin_after", "kind": "deadline"},
        )

if __name__ == "__main__":
    unittest.main()
