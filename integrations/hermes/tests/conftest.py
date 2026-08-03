from __future__ import annotations

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
PLUGIN_ROOT = ROOT / "integrations" / "hermes" / "plugin"
SDK_ROOT = ROOT / "sdk" / "python" / "src"
HERMES_ROOT = ROOT / "external" / "hermes-agent"

for path in (PLUGIN_ROOT, SDK_ROOT, HERMES_ROOT):
    value = str(path)
    if value not in sys.path:
        sys.path.insert(0, value)
