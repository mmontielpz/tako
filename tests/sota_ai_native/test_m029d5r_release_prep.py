"""M029D5R release-boundary and version-consistency tests for v0.1.0a3."""

import io
import unittest
from unittest import mock

from tools.ai_cli_providers import authorize_provider_execution, detect_ai_cli_providers
from tools.cli import __version__ as cli_version
from tools.sota_ai_native import workflow


class TestM029D5RReleaseBoundaries(unittest.TestCase):
    def test_version_is_0_1_0a3(self) -> None:
        self.assertEqual(cli_version, "0.1.0a3")

    def test_copilot_excluded_fail_closed_without_allow_unisolated_cli(self) -> None:
        ok, msg = authorize_provider_execution("copilot-cli", allow_cli_session=True, allow_paid_api=False)
        self.assertFalse(ok)
        self.assertIn("excluded", msg.lower())

        ok_override, _ = authorize_provider_execution(
            "copilot-cli", allow_cli_session=True, allow_paid_api=False, allow_unisolated_cli=True
        )
        self.assertTrue(ok_override)

        with mock.patch("sys.stderr", new_callable=io.StringIO) as err:
            rc = workflow.main([
                "--topic", "Edge AI",
                "--objective", "Test exclusion",
                "--provider", "copilot-cli",
                "--allow-cli-session",
            ])
            self.assertEqual(rc, 1)
            self.assertIn("excluded", err.getvalue().lower())

    def test_detect_ai_cli_providers_marks_copilot_excluded(self) -> None:
        specs = {p["id"]: p for p in detect_ai_cli_providers()}
        self.assertEqual(specs["copilot-cli"]["live_inference_status"], "EXCLUDED_PARTIAL_ISOLATION")
