from __future__ import annotations

import subprocess
import re
from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[3]
INTEGRATION_ROOT = ROOT / "integrations" / "hermes"
HERMES_ROOT = ROOT / "external" / "hermes-agent"


def test_bootstrap_check_pins_the_real_desktop_and_memory_provider_contract() -> None:
    lock = yaml.safe_load((INTEGRATION_ROOT / "hermes.lock.yaml").read_text(encoding="utf-8"))

    assert lock["schema_version"] == "hermes-integration-lock.v1"
    assert lock["repository"] == "https://github.com/NousResearch/hermes-agent.git"
    assert lock["branch"] == "main"
    assert len(lock["commit"]) == 40
    assert lock["desktop_path"] == "apps/desktop"

    result = subprocess.run(
        ["bash", str(INTEGRATION_ROOT / "scripts" / "bootstrap.sh"), "--check"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr
    assert lock["commit"] in result.stdout
    assert (HERMES_ROOT / "apps" / "desktop" / "package.json").is_file()
    assert (HERMES_ROOT / "agent" / "memory_provider.py").is_file()
    assert (HERMES_ROOT / "plugins" / "memory" / "__init__.py").is_file()


def test_provider_commit_matches_locked_hermes_commit() -> None:
    lock = yaml.safe_load((INTEGRATION_ROOT / "hermes.lock.yaml").read_text(encoding="utf-8"))
    provider = (INTEGRATION_ROOT / "plugin" / "jc_memory" / "provider.py").read_text(encoding="utf-8")
    match = re.search(r'HERMES_COMMIT = "([0-9a-f]{40})"', provider)
    assert match is not None
    assert match.group(1) == lock["commit"]
