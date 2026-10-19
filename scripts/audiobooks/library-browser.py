#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("library_verify", Path(__file__).with_name("library-browser-verify.py"))
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


def run(arguments, *, env, output=None, check=True):
    try:
        result = subprocess.run(arguments, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
    except subprocess.TimeoutExpired:
        raise ValueError("An owned Docker or source verification command exceeded its deadline") from None
    if len(result.stdout) > 5 * 1024 * 1024:
        raise ValueError("The owned subprocess output exceeded its bound")
    if output:
        output.write_bytes(result.stdout)
        output.chmod(0o600)
    if check and result.returncode:
        raise ValueError("An owned Docker or source verification command failed")
    return result


def main():
    os.umask(0o077)
    engines = sys.argv[1:] or ["chromium", "webkit"]
    if len(engines) > 2 or len(set(engines)) != len(engines) or any(value not in ["chromium", "webkit"] for value in engines):
        raise ValueError("Choose chromium, webkit, or both")
    env = os.environ.copy()
    image, source, inputs = (env.get(key, "") for key in ["HITO_PACKAGED_IMAGE", "HITO_PACKAGED_SOURCE", "HITO_PACKAGED_INPUTS"])
    if not re.fullmatch(r"sha256:[a-f0-9]{64}", image) or not re.fullmatch(r"[a-f0-9]{40}", source) or not re.fullmatch(r"[a-f0-9]{64}", inputs):
        raise ValueError("Exact immutable packaged identities are required")
    directory = Path(tempfile.mkdtemp(prefix="hito-library-browser-"))
    print(json.dumps({"event": "audiobook_library.gate_started", "evidence": str(directory), "source": source, "image": image}), flush=True)
    helper = Path(env.get("HITO_PACKAGED_PIN_HELPER", str(ROOT / "scripts/artwork/source-pin.py")))
    def pin(name):
        run([sys.executable, str(helper), "--root", str(ROOT), "--expect-head", source, "--expect-digest", inputs, "--output", str(directory / name)], env=env, output=directory / f"{name}.log")
    pin("source-before.json")
    inspect = run(["docker", "image", "inspect", image], env=env, output=directory / "image.json")
    if not verify.image_contract(json.loads(inspect.stdout)[0], image, source, inputs):
        raise ValueError("The packaged image does not match the frozen product inputs")
    tools = ["docker", "compose", "-p", "hito-spotify-albums-tools", "-f", "compose.yaml", "-f", "scripts/audiobooks/library-browser.tools.yaml", "--profile", "tools", "run", "-T", "--rm", "--no-deps", "--pull", "never", "node"]
    run(tools + ["node", "/workspace/scripts/audio-fixtures.mjs", "--e2e"], env=env, output=directory / "fixtures.log")
    summaries = []
    for engine in engines:
        for workflow in ["traversal", "recovery", "privacy"]:
            case_dir = directory / f"{engine}-{workflow}"
            case_dir.mkdir()
            browser_evidence = case_dir / "browser-evidence"
            browser_evidence.mkdir(mode=0o777)
            browser_evidence.chmod(0o777)
            identity = uuid.uuid4().hex
            project = f"hito-library-{uuid.uuid4().hex[:12]}"
            browser = f"{project}-browser"
            env.update(HITO_LIBRARY_RUN_ID=identity, HITO_LIBRARY_WORKTREE=str(ROOT), HITO_LIBRARY_EVIDENCE=str(browser_evidence))
            compose = ["docker", "compose", "-p", project, "-f", "scripts/audiobooks/library-browser.compose.yaml", "--profile", "e2e"]
            failure = None
            try:
                run(compose + ["config", "--format", "json"], env=env, output=case_dir / "compose.json")
                run(compose + ["up", "-d", "--no-build", "--pull", "never", "--wait", "library-server"], env=env, output=case_dir / "start.log")
                container = run(compose + ["ps", "-q", "library-server"], env=env).stdout.decode().strip()
                inspected = run(["docker", "inspect", container], env=env, output=case_dir / "container.json")
                if not verify.container_contract(json.loads(inspected.stdout)[0], image, project, ROOT / "fixtures/e2e-generated"):
                    raise ValueError("The owned runtime isolation contract changed")
                test_file = "e2e/audiobook-library-privacy.spec.ts" if workflow == "privacy" else "e2e/audiobook-library.spec.ts"
                tested = run(compose + ["run", "-T", "--name", browser, "--no-deps", "--pull", "never", "library-browser", "node", "node_modules/@playwright/test/cli.js", "test", test_file, "--grep", workflow, "--browser", engine, "--output", "/tmp/hito-library-results"], env=env, output=case_dir / "browser.log", check=False)
                for target in ["canaries.json", "summary.json", "privacy-findings.json"]:
                    (case_dir / target).write_bytes((browser_evidence / target).read_bytes())
                    (case_dir / target).chmod(0o600)
                if tested.returncode:
                    raise ValueError("The owned browser case failed; inspect its private numeric summary")
            except (ValueError, OSError, KeyError, json.JSONDecodeError) as error:
                failure = error
            finally:
                cleanup = [
                    (compose + ["stop", "-t", "15", "library-server"], "stop.log"),
                    (compose + ["logs", "--no-color", "--no-log-prefix", "library-server"], "server.log"),
                    (["docker", "rm", "-f", browser], "remove-browser.log"),
                    (compose + ["down", "--timeout", "15"], "cleanup.log"),
                ]
                for arguments, filename in cleanup:
                    try:
                        run(arguments, env=env, output=case_dir / filename, check=False)
                    except (ValueError, OSError):
                        failure = failure or ValueError("The owned fixture cleanup or evidence capture failed")
            if failure:
                raise failure
            summaries.append(verify.evidence(case_dir, workflow, engine))
            print(json.dumps({"event": "audiobook_library.case_passed", "browser": engine, "workflow": workflow}), flush=True)
    pin("source-after.json")
    if (directory / "source-before.json").read_bytes() != (directory / "source-after.json").read_bytes():
        raise ValueError("The frozen product source changed during browser acceptance")
    (directory / "qualified.json").write_text(json.dumps({"image": image, "source": source, "product_inputs_sha256": inputs, "cases": summaries}))
    print(json.dumps({"event": "audiobook_library.gate_passed", "evidence": str(directory), "cases": len(summaries)}), flush=True)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, json.JSONDecodeError):
        print(json.dumps({"event": "audiobook_library.gate_failed", "reason": "owned_fixture_or_evidence"}), flush=True)
        raise SystemExit(1)
