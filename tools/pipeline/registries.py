"""The JSON registries the catalog reads: suites, test inputs, profiles, GLES checks."""

import json
from pathlib import Path

DATA = Path(__file__).parent
PROFILES = json.loads((DATA / "profiles.json").read_text())
SUITES = json.loads((DATA / "suites.json").read_text())
TEST_INPUTS = json.loads((DATA / "test-inputs.json").read_text())
GLES_CHECKS = json.loads((DATA / "gles.json").read_text())
