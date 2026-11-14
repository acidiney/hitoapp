import hashlib
from contextlib import redirect_stdout
import importlib.util
from io import StringIO
import json
from pathlib import Path
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
        with patch.object(RUNNER, "Gate", FailedModuleGate), redirect_stdout(output):
            status = RUNNER.main()
        self.assertEqual(status, 1)
        result = json.loads(output.getvalue())
        self.assertEqual(
            result.get("failure"),
            {"phase": "module_pin_before", "kind": "command_exit", "exit_code": 17},
        )
        self.assertNotIn(sentinel, output.getvalue())

if __name__ == "__main__":
    unittest.main()
