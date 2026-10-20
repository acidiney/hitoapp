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

ROOT = Path(__file__).resolve().parents[3]
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("choice_verify", Path(__file__).with_name("verify.py"))
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)
BROWSER = "mcr.microsoft.com/playwright:v1.56.1-noble@sha256:f1e7e01021efd65dd1a2c56064be399f3e4de00fd021ac561325f2bfbb2b837a"


def run(arguments, env, output=None, check=True):
    try:
        result = subprocess.run(arguments, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
    except subprocess.TimeoutExpired:
        raise ValueError("An owned chooser command exceeded its deadline") from None
    if len(result.stdout) > 5 * 1024 * 1024:
        raise ValueError("An owned chooser command exceeded its output bound")
    if output:
        output.write_bytes(result.stdout)
        output.chmod(0o600)
    if check and result.returncode:
        raise ValueError("An owned chooser command failed")
    return result


def run_tool(arguments, env, output=None):
    run_id = uuid.uuid4().hex
    project = f"hito-lyric-choice-tools-{run_id}"
    name = f"{project}-node"
    labels = {"com.docker.compose.project": project,
              "org.hito.fixture-run": run_id,
              "org.hito.owned-fixture": "lyric-provider-choice-tools"}
    tools = ["docker", "compose", "-p", project, "-f", "scripts/lyrics/provider-choice/tools.yaml",
             "--profile", "tools", "run", "-T", "--rm", "--name", name,
             "--label", f"org.hito.fixture-run={run_id}",
             "--label", "org.hito.owned-fixture=lyric-provider-choice-tools",
             "--no-deps", "--pull", "never", "node"]
    try:
        return run(tools + arguments, env, output)
    finally:
        found = run(["docker", "inspect", name], env, check=False)
        if found.returncode == 0:
            containers = json.loads(found.stdout)
            if len(containers) != 1 or containers[0].get("Name") != f"/{name}" or any(
                    containers[0].get("Config", {}).get("Labels", {}).get(key) != value
                    for key, value in labels.items()):
                raise ValueError("The exact chooser tool ownership changed")
            run(["docker", "rm", "-f", containers[0]["Id"]], env, check=False)
        filters = [argument for key, value in labels.items() for argument in ["--filter", f"label={key}={value}"]]
        remaining = run(["docker", "ps", "-aq", *filters], env)
        if remaining.stdout.strip():
            raise ValueError("An exact owned chooser tool survived cleanup")


def main():
    os.umask(0o077)
    engines = sys.argv[1:] or ["chromium"]
    if len(engines) > 2 or len(set(engines)) != len(engines) or any(value not in ["chromium", "webkit"] for value in engines):
        raise ValueError("Choose chromium, webkit, or both")
    env = os.environ.copy()
    image, source, inputs = (env.get(key, "") for key in ["HITO_PACKAGED_IMAGE", "HITO_PACKAGED_SOURCE", "HITO_PACKAGED_INPUTS"])
    if not re.fullmatch(r"sha256:[a-f0-9]{64}", image) or source != "6cc3a0a42fcbde7bb590bcd0de644a5aa8cefa23" or not re.fullmatch(r"[a-f0-9]{64}", inputs):
        raise ValueError("The new chooser source and exact immutable image identities are required")
    directory = Path(tempfile.mkdtemp(prefix="hito-lyric-provider-choice-"))
    print(json.dumps({"event": "lyric_provider_choice.gate_started", "evidence": str(directory), "source": source, "image": image}), flush=True)
    frozen = verify.overlay(ROOT)
    (directory / "gate-source.json").write_text(json.dumps(frozen, indent=2))

    def pin(name):
        run([sys.executable, str(ROOT / "scripts/artwork/source-pin.py"), "--root", str(ROOT), "--expect-head", source, "--expect-digest", inputs, "--output", str(directory / name)], env, directory / f"{name}.log")

    pin("source-before.json")
    inspected = run(["docker", "image", "inspect", image], env, directory / "image.json")
    if not verify.image_contract(json.loads(inspected.stdout)[0], image, source, inputs):
        raise ValueError("The new chooser runtime does not match its product source")
    browser_image = json.loads(run(["docker", "image", "inspect", BROWSER], env, directory / "browser-image.json").stdout)[0]["Id"]
    run_tool(["node", "/workspace/scripts/audio-fixtures.mjs", "--e2e"], env, directory / "fixtures.log")
    modules_before = run_tool(["node", "/workspace/scripts/lyrics/provider-choice/module-pin.mjs"], env, directory / "modules-before.log").stdout.decode().splitlines()[-1]
    modules = json.loads(modules_before)
    summaries = []
    for engine in engines:
        case = directory / engine
        case.mkdir()
        browser_evidence = case / "browser-evidence"
        browser_evidence.mkdir(mode=0o777)
        browser_evidence.chmod(0o777)
        project = f"hito-lyric-choice-{uuid.uuid4().hex[:12]}"
        browser = f"{project}-browser"
        run_id = uuid.uuid4().hex
        env.update(HITO_LYRIC_CHOICE_RUN_ID=run_id, HITO_LYRIC_CHOICE_WORKTREE=str(ROOT), HITO_LYRIC_CHOICE_EVIDENCE=str(browser_evidence))
        compose = ["docker", "compose", "-p", project, "-f", "scripts/lyrics/provider-choice/compose.yaml", "--profile", "e2e"]
        failure = False
        try:
            config = json.loads(run(compose + ["config", "--format", "json"], env, case / "compose.json").stdout)
            if config.get("networks") or config["services"]["app"]["network_mode"] != "none" or config["services"]["browser"]["network_mode"] != "service:app":
                raise ValueError("The chooser may not create a bridge network")
            run(compose + ["up", "-d", "--no-build", "--pull", "never", "--wait", "app"], env, case / "start.log")
            app = run(compose + ["ps", "-q", "app"], env).stdout.decode().strip()
            inspected = json.loads(run(["docker", "inspect", app], env, case / "app.json").stdout)[0]
            if not verify.runtime_contract(inspected, image, project, run_id, ROOT):
                raise ValueError("The chooser app isolation changed")
            tested = run(compose + ["run", "-T", "--name", browser, "--no-deps", "--pull", "never", "browser", "node", "node_modules/@playwright/test/cli.js", "test", "--config", "playwright.lyric-provider-choice.config.ts", "--browser", engine, "--output", "/tmp/hito-lyric-choice-results"], env, case / "browser.log", check=False)
            inspected = json.loads(run(["docker", "inspect", browser], env, case / "browser.json").stdout)[0]
            if not verify.runtime_contract(inspected, browser_image, project, run_id, ROOT, browser=True, app=app, evidence=browser_evidence):
                raise ValueError("The chooser browser isolation changed")
            for name in ["canaries.json", "summary.json"]:
                (case / name).write_bytes((browser_evidence / name).read_bytes())
                (case / name).chmod(0o600)
            if tested.returncode:
                raise ValueError("The chooser tracer failed; inspect its private numeric checkpoint")
        except (ValueError, OSError, KeyError, json.JSONDecodeError):
            failure = True
        finally:
            commands = [(compose + ["stop", "-t", "15", "app"], "stop.log"),
                        (compose + ["logs", "--no-color", "--no-log-prefix", "app"], "server.log")]
            try:
                found = run(["docker", "inspect", browser], env, check=False)
                if found.returncode == 0:
                    labels = json.loads(found.stdout)[0]["Config"]["Labels"]
                    if labels.get("com.docker.compose.project") != project or labels.get("org.hito.fixture-run") != run_id or labels.get("org.hito.owned-fixture") != "lyric-provider-choice":
                        raise ValueError("The exact chooser browser ownership changed")
                    commands.append((["docker", "rm", "-f", browser], "remove-browser.log"))
            except (ValueError, OSError, KeyError, json.JSONDecodeError):
                failure = True
            commands.append((compose + ["down", "--timeout", "15"], "cleanup.log"))
            for arguments, name in commands:
                try:
                    if run(arguments, env, case / name, check=False).returncode:
                        failure = True
                except (ValueError, OSError):
                    failure = True
        if failure:
            raise ValueError("The owned chooser fixture or tracer failed")
        summaries.append(verify.evidence(case, engine))
        print(json.dumps({"event": "lyric_provider_choice.case_passed", "browser": engine, "checkpoint": 10}), flush=True)
    modules_after = json.loads(run_tool(["node", "/workspace/scripts/lyrics/provider-choice/module-pin.mjs"], env, directory / "modules-after.log").stdout.decode().splitlines()[-1])
    pin("source-after.json")
    if modules_after != modules or (directory / "source-before.json").read_bytes() != (directory / "source-after.json").read_bytes() or verify.overlay(ROOT) != frozen:
        raise ValueError("The frozen product, gate or browser modules changed")
    (directory / "qualified.json").write_text(json.dumps({"source": source, "image": image, "product_inputs_sha256": inputs, "browser_image": browser_image, "modules": modules, "gate_source": frozen, "cases": summaries}, indent=2))
    print(json.dumps({"event": "lyric_provider_choice.gate_passed", "evidence": str(directory), "cases": len(summaries)}), flush=True)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, json.JSONDecodeError):
        print(json.dumps({"event": "lyric_provider_choice.gate_failed", "reason": "owned_fixture_or_evidence"}), flush=True)
        raise SystemExit(1)
