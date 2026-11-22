"""Tests for M024D Adaptive Research Setup Wizard, profiles, config security, and precedence."""

from __future__ import annotations

import io
import json
import os
import pathlib
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest import mock

from tools.cli import main as cli_main
from tools.researcher_setup import (
    ACCESS_PROFILES,
    SetupConfigError,
    get_user_config_path,
    load_user_config,
    resolve_effective_discovery_settings,
    save_user_config,
    validate_user_config,
)


class ResearcherSetupTests(unittest.TestCase):
    def test_windows_and_linux_config_paths(self):
        win_p = get_user_config_path(platform_name="nt", env={"APPDATA": "/win/AppData/Roaming"})
        self.assertEqual(str(win_p), "/win/AppData/Roaming/tako/config.json")
        lin_p = get_user_config_path(platform_name="posix", env={"XDG_CONFIG_HOME": "/home/u/.xdg"})
        self.assertEqual(str(lin_p), "/home/u/.xdg/tako/config.json")
        ovr_p = get_user_config_path(platform_name="posix", env={"TAKO_CONFIG_PATH": "/tmp/custom_tako.json"})
        self.assertEqual(str(ovr_p), "/tmp/custom_tako.json")

    def test_config_validation_rejects_secrets_and_unimplemented_providers(self):
        with self.assertRaises(SetupConfigError):
            validate_user_config({"profile": "free", "sources": ["openalex"], "api_key": "12345"})
        with self.assertRaises(SetupConfigError):
            validate_user_config({"profile": "free", "sources": ["openalex"], "note": "sk-secret1234567890"})
        with self.assertRaises(SetupConfigError):
            validate_user_config({"profile": "institutional", "sources": ["scopus"]})
        with self.assertRaises(SetupConfigError):
            validate_user_config({"profile": "free", "sources": ["google-scholar-assisted"]})
        with self.assertRaises(SetupConfigError):
            validate_user_config({"profile": "offline", "sources": ["openalex"]})

    def test_non_interactive_profiles_and_no_silent_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg_file = pathlib.Path(tmp) / "tako" / "config.json"
            with mock.patch.dict(os.environ, {"TAKO_CONFIG_PATH": str(cfg_file)}):
                for prof in ("free", "hybrid", "institutional", "paid", "offline"):
                    with redirect_stdout(io.StringIO()):
                        rc = cli_main(["setup", "--profile", prof, "--non-interactive", "--force", "--json"])
                    self.assertEqual(rc, 0)
                    loaded = load_user_config(cfg_file)
                    self.assertIsNotNone(loaded)
                    self.assertEqual(loaded["profile"], prof)
                    self.assertFalse(loaded["allow_paid_operations"])

                # Without --force, existing config must not be overwritten silently
                with redirect_stderr(io.StringIO()):
                    rc_conflict = cli_main(["setup", "--profile", "free", "--non-interactive"])
                self.assertEqual(rc_conflict, 2)
                self.assertEqual(load_user_config(cfg_file)["profile"], "offline")

    def test_interactive_setup_simulation_and_show(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg_file = pathlib.Path(tmp) / "config.json"
            with mock.patch.dict(os.environ, {"TAKO_CONFIG_PATH": str(cfg_file)}):
                with mock.patch("builtins.input", side_effect=["2", "openalex,arxiv", "copilot"]):
                    buf = io.StringIO()
                    with redirect_stdout(buf):
                        rc = cli_main(["setup"])
                self.assertEqual(rc, 0)
                self.assertIn("SETUP_COMPLETE", buf.getvalue())
                loaded = load_user_config(cfg_file)
                self.assertEqual(loaded["profile"], "hybrid")
                self.assertEqual(loaded["sources"], ["openalex", "arxiv"])
                self.assertEqual(loaded["assistant"], "copilot")

                show_buf = io.StringIO()
                with redirect_stdout(show_buf):
                    rc_show = cli_main(["setup", "--show", "--json"])
                self.assertEqual(rc_show, 0)
                shown = json.loads(show_buf.getvalue())
                self.assertTrue(shown["configured"])
                self.assertEqual(shown["config"]["profile"], "hybrid")

    def test_providers_list_missing_credentials_and_no_secret_leak(self):
        with mock.patch.dict(os.environ, {
            "OPENAI_API_KEY": "sk-topsecret-never-leak-111",
            "SEMANTIC_SCHOLAR_API_KEY": "s2-topsecret-never-leak-222",
        }):
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = cli_main(["providers", "list", "--json"])
            self.assertEqual(rc, 0)
            raw_out = buf.getvalue()
            self.assertNotIn("sk-topsecret-never-leak-111", raw_out)
            self.assertNotIn("s2-topsecret-never-leak-222", raw_out)
            rows = {r["provider_id"]: r for r in json.loads(raw_out)["providers"]}
            self.assertTrue(rows["openalex"]["implemented"])
            self.assertTrue(rows["openalex"]["available"])
            self.assertTrue(rows["scopus"]["documented_only"])
            self.assertFalse(rows["scopus"]["available"])
            self.assertTrue(rows["google-scholar-assisted"]["manual_assisted"])

            doc_buf = io.StringIO()
            with redirect_stdout(doc_buf):
                rc_doc = cli_main(["doctor"])
            self.assertEqual(rc_doc, 0)
            self.assertNotIn("s2-topsecret-never-leak-222", doc_buf.getvalue())

    def test_paid_profile_refusal_and_offline_network_refusal(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg_file = pathlib.Path(tmp) / "config.json"
            with mock.patch.dict(os.environ, {"TAKO_CONFIG_PATH": str(cfg_file), "OPENAI_API_KEY": "sk-dummy-1234567890"}):
                with redirect_stdout(io.StringIO()):
                    self.assertEqual(cli_main(["setup", "--profile", "paid", "--non-interactive"]), 0)
                # Paid profile does NOT authorize paid API calls without --allow-paid-api
                with redirect_stderr(io.StringIO()) as err:
                    rc_paid = cli_main(["sota-ai", "--topic", "GNN", "--objective", "Survey", "--provider", "openai-responses"])
                self.assertEqual(rc_paid, 1)
                self.assertIn("PROVIDER_BLOCKED", err.getvalue())

                # Offline profile blocks live sota and live sota-ai
                with redirect_stdout(io.StringIO()):
                    self.assertEqual(cli_main(["setup", "--profile", "offline", "--non-interactive", "--force"]), 0)
                with redirect_stderr(io.StringIO()) as err2:
                    rc_off1 = cli_main(["sota", "--topic", "GNN", "--objective", "Survey", "--iterative"])
                self.assertEqual(rc_off1, 2)
                self.assertIn("OFFLINE_MODE_BLOCKED", err2.getvalue())

                with redirect_stderr(io.StringIO()) as err3:
                    rc_off2 = cli_main(["sota-ai", "--topic", "GNN", "--objective", "Survey", "--provider", "copilot-cli", "--allow-paid-api"])
                self.assertEqual(rc_off2, 2)
                self.assertIn("OFFLINE_MODE_BLOCKED", err3.getvalue())

    def test_configuration_precedence_cli_over_config_over_defaults(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg_file = pathlib.Path(tmp) / "config.json"
            save_user_config({
                "profile": "free",
                "sources": ["openalex", "arxiv"],
                "assistant": "agy",
                "default_max_http_requests": 15,
            }, cfg_file)

            eff_cfg = resolve_effective_discovery_settings(config_path=cfg_file)
            self.assertEqual(eff_cfg["sources"], ["openalex", "arxiv"])
            self.assertEqual(eff_cfg["sources_origin"], "user_config")
            self.assertEqual(eff_cfg["max_http_requests"], 15)
            self.assertEqual(eff_cfg["max_http_requests"] and eff_cfg["max_http_origin"], "user_config")

            eff_cli = resolve_effective_discovery_settings(cli_sources="crossref", cli_max_http=7, config_path=cfg_file)
            self.assertEqual(eff_cli["sources"], ["crossref"])
            self.assertEqual(eff_cli["sources_origin"], "cli")
            self.assertEqual(eff_cli["max_http_requests"], 7)
            self.assertEqual(eff_cli["max_http_origin"], "cli")


if __name__ == "__main__":
    unittest.main()
