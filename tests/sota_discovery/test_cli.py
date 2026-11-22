"""CLI tests for unified tako entry point."""

import io
import os
import unittest
from contextlib import redirect_stdout
from unittest import mock

from tools.cli import main


class TakoCliTests(unittest.TestCase):
    def test_version_and_help(self):
        buf = io.StringIO()
        with self.assertRaises(SystemExit) as ctx:
            with redirect_stdout(buf):
                main(["--version"])
        self.assertEqual(ctx.exception.code, 0)
        self.assertIn("0.0.1.dev3", buf.getvalue())

        buf2 = io.StringIO()
        with self.assertRaises(SystemExit) as ctx2:
            with redirect_stdout(buf2):
                main(["--help"])
        self.assertEqual(ctx2.exception.code, 0)
        self.assertIn("doctor", buf2.getvalue())
        self.assertIn("sota-ai", buf2.getvalue())

    def test_doctor_reports_status_without_leaking_secrets(self):
        buf = io.StringIO()
        with mock.patch.dict(os.environ, {"OPENAI_API_KEY": "sk-secret-do-not-leak-123456"}):
            with redirect_stdout(buf):
                rc = main(["doctor"])
        self.assertEqual(rc, 0)
        out = buf.getvalue()
        self.assertNotIn("sk-secret-do-not-leak", out)
        self.assertIn("INSTALLED=", out)
        self.assertIn("CONFIGURED=", out)
        self.assertIn("AVAILABLE=", out)
        self.assertIn("LIVE_TESTED=", out)
        self.assertIn("copilot-cli", out)


if __name__ == "__main__":
    unittest.main()
