"""Offline Researcher Experience regression suite (RX01-RX20 + M022A-R07/R08) for P4 M023."""

from __future__ import annotations

import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

from tools.cli import main as cli_main
from tools.research_workspace import WORKSPACE_FILENAME, ingest_local_evidence
from tools.sota_evidence import (
    COMP_DIRECT,
    REPORT_EXPLORATORY,
    acquire_evidence,
    compare_papers,
    normalize_extraction,
)
from tools.sota_integrity import BIB_STATUS_VERIFIED


class CliProductExperienceTests(unittest.TestCase):
    def test_rx01_rx02_command_and_capability_discovery(self):
        buf = io.StringIO()
        with self.assertRaises(SystemExit) as ctx:
            with redirect_stdout(buf):
                cli_main(["--version"])
        self.assertEqual(ctx.exception.code, 0)
        self.assertIn("0.1.0a2", buf.getvalue())

        buf_doc = io.StringIO()
        with mock.patch.dict(os.environ, {"OPENAI_API_KEY": "sk-secret-rx18-never-leak"}):
            with redirect_stdout(buf_doc):
                rc = cli_main(["doctor"])
        self.assertEqual(rc, 0)
        out = buf_doc.getvalue()
        self.assertNotIn("sk-secret-rx18-never-leak", out)
        self.assertIn("AUTHENTICATED=", out)
        self.assertIn("evidence_pdf: UNSUPPORTED", out)
        self.assertIn("windows_native: NOT_TESTED", out)

    def test_rx03_rx15_rx18_workspace_init_status_and_no_secret_leakage(self):
        with tempfile.TemporaryDirectory() as tmp:
            ws = Path(tmp) / "study"
            with mock.patch.dict(os.environ, {"OPENAI_API_KEY": "sk-secret-rx18-ws"}):
                rc = cli_main([
                    "init", "--topic", "Quantum Reservoir Computing",
                    "--objective", "Benchmark temporal tasks",
                    "--workspace", str(ws),
                ])
                self.assertEqual(rc, 0)
                manifest_text = (ws / WORKSPACE_FILENAME).read_text(encoding="utf-8")
                self.assertNotIn("sk-secret-rx18-ws", manifest_text)
                manifest = json.loads(manifest_text)
                self.assertEqual(manifest["topic"], "Quantum Reservoir Computing")
                self.assertEqual(manifest["objective"], "Benchmark temporal tasks")
                self.assertFalse(manifest["provider_config"]["credentials_stored"])

                buf = io.StringIO()
                with redirect_stdout(buf):
                    rc_st = cli_main(["status", "--workspace", str(ws)])
                self.assertEqual(rc_st, 0)
                self.assertIn("Quantum Reservoir Computing", buf.getvalue())
                self.assertIn("evidence: NOT_STARTED", buf.getvalue())

    def test_rx04_rx05_rx20_end_to_end_offline_journey_and_interoperability(self):
        with tempfile.TemporaryDirectory() as tmp:
            ws = Path(tmp) / "journey"
            self.assertEqual(cli_main([
                "init", "--topic", "Graph Neural Networks",
                "--objective", "Compare molecular property predictors",
                "--workspace", str(ws),
            ]), 0)

            # RX04: iterative discovery -> workspace evidence
            fx = Path(tmp) / "fx.json"
            fx.write_text(json.dumps({
                "results": [{
                    "id": "https://openalex.org/W1",
                    "doi": "https://doi.org/10.1000/gnn.rx04",
                    "title": "Equivariant GNNs on QM9",
                    "publication_year": 2024,
                    "primary_location": {"source": {"display_name": "NeurIPS"}},
                    "authorships": [{"author": {"display_name": "Ada Lovelace"}}],
                    "abstract_inverted_index": {"Equivariant": [0], "GNN": [1], "QM9.": [2]},
                    "referenced_works": [],
                    "cited_by_count": 3,
                }]
            }), encoding="utf-8")
            disc_dir = ws / "runs" / "discovery"
            self.assertEqual(cli_main([
                "sota", "--topic", "Graph Neural Networks",
                "--objective", "Compare molecular property predictors",
                "--iterative", "--fixture", str(fx), "--out-dir", str(disc_dir),
            ]), 0)

            note = Path(tmp) / "note.md"
            note.write_text("# Local Observation\nEquivariant GNNs report 0.010 eV MAE on QM9.\n", encoding="utf-8")

            self.assertEqual(cli_main([
                "evidence", "--workspace", str(ws),
                "--from-workflow-dir", str(disc_dir),
                "--source", str(note),
            ]), 0)

            q = json.loads((ws / "runs" / "evidence" / "quality_measurements.json").read_text(encoding="utf-8"))
            self.assertEqual(q["report_qualification"], REPORT_EXPLORATORY)

            # RX14: portable export
            exp_dir = ws / "exports" / "pack1"
            self.assertEqual(cli_main([
                "export", "--workspace", str(ws), "--out-dir", str(exp_dir), "--format", "all",
            ]), 0)
            self.assertTrue((exp_dir / "artifact_inventory.json").is_file())
            self.assertTrue((exp_dir / "references.ris").is_file())
            self.assertTrue((exp_dir / "sota_report.md").is_file())

            # RX05: AI-native fixture -> evidence
            ai_dir = Path(tmp) / "ai_out"
            self.assertEqual(cli_main([
                "sota-ai", "--topic", "Graph Neural Networks",
                "--objective", "Compare molecular property predictors",
                "--provider", "fixture", "--out-dir", str(ai_dir),
            ]), 0)
            ev_ai = Path(tmp) / "ev_ai"
            self.assertEqual(cli_main(["evidence", "--from-workflow-dir", str(ai_dir), "--out-dir", str(ev_ai)]), 0)
            self.assertTrue((ev_ai / "sota_report.md").is_file())

    def test_rx06_rx07_local_txt_md_bib_ris_jats_ingestion(self):
        with tempfile.TemporaryDirectory() as tmp:
            t = Path(tmp)
            txt_f = t / "paper.txt"
            txt_f.write_text("Plain text observation on QM9.", encoding="utf-8")
            md_f = t / "notes.md"
            md_f.write_text("# Study\nMarkdown summary.", encoding="utf-8")
            bib_f = t / "refs.bib"
            bib_f.write_text("@article{k1,\n  title={Bib Paper},\n  author={Ada Lovelace},\n  year={2024},\n  doi={10.1000/bib1}\n}\n", encoding="utf-8")
            ris_f = t / "refs.ris"
            ris_f.write_text("TY  - JOUR\nTI  - RIS Paper\nAU  - Alan Turing\nPY  - 2024\nDO  - 10.1000/ris1\nER  - \n", encoding="utf-8")
            xml_f = t / "article.xml"
            xml_f.write_text("<article><front><article-meta><title-group><article-title>JATS Paper</article-title></title-group><abstract><p>Abs text</p></abstract></article-meta></front><body><p>Body text</p></body></article>", encoding="utf-8")

            papers, recs = ingest_local_evidence([txt_f, md_f, bib_f, ris_f, xml_f])
            self.assertEqual(len(papers), 5)
            self.assertEqual(len(recs), 5)
            self.assertEqual(papers[2]["Bibliographic status"], BIB_STATUS_VERIFIED)
            self.assertEqual(papers[3]["Bibliographic status"], BIB_STATUS_VERIFIED)
            self.assertEqual(papers[4]["Title"], "JATS Paper")

    def test_rx08_rx09_unsupported_pdf_malformed_and_safety_checks(self):
        with tempfile.TemporaryDirectory() as tmp:
            t = Path(tmp)
            pdf_f = t / "paper.pdf"
            pdf_f.write_bytes(b"%PDF-1.4 fake")
            with self.assertRaises(ValueError) as ctx:
                ingest_local_evidence([pdf_f])
            self.assertIn("Unsupported format '.pdf'", str(ctx.exception))

            bad_json = t / "bad.json"
            bad_json.write_text("{not valid json", encoding="utf-8")
            with self.assertRaises(ValueError):
                ingest_local_evidence([bad_json])

            bad_xml = t / "xxe.xml"
            bad_xml.write_text("<!DOCTYPE foo [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><article/>", encoding="utf-8")
            with self.assertRaises(ValueError):
                ingest_local_evidence([bad_xml])

            link_f = t / "link.txt"
            link_f.symlink_to(pdf_f)
            with self.assertRaises(ValueError):
                ingest_local_evidence([link_f])

    def test_rx11_rx12_rx13_collision_protection_recovery_and_repeatability(self):
        with tempfile.TemporaryDirectory() as tmp:
            ws = Path(tmp) / "ws"
            self.assertEqual(cli_main(["init", "--topic", "T", "--objective", "O", "--workspace", str(ws)]), 0)
            # Collision on init without --force
            self.assertEqual(cli_main(["init", "--topic", "T", "--objective", "O", "--workspace", str(ws)]), 2)

            src = Path(tmp) / "ev.txt"
            src.write_text("Observation v1.", encoding="utf-8")
            out_dir = ws / "runs" / "evidence"
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws), "--source", str(src), "--out-dir", str(out_dir)]), 0)

            # Collision on non-empty out_dir without --force or --resume
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws), "--source", str(src), "--out-dir", str(out_dir)]), 2)

            # Modify input -> --resume must refuse stale manifest
            src.write_text("Observation v2 modified.", encoding="utf-8")
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws), "--source", str(src), "--out-dir", str(out_dir), "--resume"]), 2)

            # Export collision protection
            exp_dir = Path(tmp) / "exp"
            self.assertEqual(cli_main(["export", "--workspace", str(ws), "--out-dir", str(exp_dir)]), 0)
            self.assertEqual(cli_main(["export", "--workspace", str(ws), "--out-dir", str(exp_dir)]), 2)
            self.assertEqual(cli_main(["export", "--workspace", str(ws), "--out-dir", str(exp_dir), "--force"]), 0)

    def test_rx16_m022a_r07_unverified_fields_never_directly_comparable(self):
        # One field matches source text ("QM9") -> mixed_unverified_fields, not author_reported
        p1 = {
            "paper_id": "m1", "Title": "P1", "Year": 2024, "DOI or source URL": "https://doi.org/10.1000/m1",
            "Bibliographic status": BIB_STATUS_VERIFIED,
            "full_text": "We evaluate on QM9.",
            "extraction": {
                "research_problem": "mol", "proposed_contributions": "A", "methodology": "M1",
                "datasets_or_materials": "QM9", "evaluation_protocol": "std", "baselines": "B",
                "experimental_conditions": "cond", "environment": "GPU", "results": "0.1",
                "author_limitations": "lim", "reported_metrics": [{"name": "MAE", "value": 0.1, "unit": "eV"}],
            },
        }
        p2 = dict(p1, paper_id="m2", Title="P2")
        ev1, ev2 = acquire_evidence(p1), acquire_evidence(p2)
        ext1, ext2 = normalize_extraction(p1, ev1), normalize_extraction(p2, ev2)
        self.assertEqual(ext1["provenance"], "mixed_unverified_fields")
        comps = compare_papers([p1, p2], {"m1": ext1, "m2": ext2}, {"m1": ev1, "m2": ev2})
        self.assertNotEqual(comps[0]["comparability_class"], COMP_DIRECT)
        self.assertFalse(comps[0]["eligible_to_publish_numerical_comparisons"])
        self.assertEqual(comps[0]["structural_compatibility"], "STRUCTURALLY_COMPATIBLE")

    def test_rx17_m022a_r08_long_local_excerpt_not_verified_complete_publication(self):
        with tempfile.TemporaryDirectory() as tmp:
            long_txt = Path(tmp) / "long_excerpt.txt"
            long_txt.write_text("Long partial excerpt. " * 60, encoding="utf-8")  # > 1200 chars
            p = {
                "paper_id": "long1",
                "Title": "Long Excerpt",
                "local_text_path": str(long_txt),
                "document_completeness": "FULL_PUBLICATION",  # caller-declared without verification
            }
            ev = acquire_evidence(p)
            self.assertNotEqual(ev["document_completeness"], "VERIFIED_COMPLETE_PUBLICATION")
            self.assertEqual(ev["document_completeness"], "DECLARED_COMPLETE_UNVERIFIED")

            p2 = {"paper_id": "long2", "Title": "Long Excerpt 2", "local_text_path": str(long_txt)}
            ev2 = acquire_evidence(p2)
            self.assertEqual(ev2["document_completeness"], "PARTIAL_EXCERPT")

    def test_m023_r01_workspace_conflict_and_directory_portability(self):
        import shutil
        with tempfile.TemporaryDirectory() as tmp:
            t = Path(tmp)
            ws1 = t / "ws_orig"
            self.assertEqual(cli_main(["init", "--topic", "Quantum Computing", "--objective", "Survey QRC", "--workspace", str(ws1)]), 0)

            # Conflicting topic in imported bundle must fail closed
            bad_bundle = t / "conflict.json"
            bad_bundle.write_text(json.dumps({"topic": "Different Topic", "objective": "Other", "papers": []}), encoding="utf-8")
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws1), "--input", str(bad_bundle)]), 2)

            # Valid local evidence inside workspace -> move workspace -> status, resume, export work
            ev_dir = ws1 / "evidence"
            src = ev_dir / "note.md"
            src.write_text("# QRC Note\nReservoir accuracy 95%.\n", encoding="utf-8")
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws1), "--source", str(src)]), 0)

            ws2 = t / "ws_moved"
            shutil.move(str(ws1), str(ws2))
            self.assertEqual(cli_main(["status", "--workspace", str(ws2)]), 0)
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws2), "--source", str(ws2 / "evidence" / "note.md"), "--resume"]), 0)
            self.assertEqual(cli_main(["export", "--workspace", str(ws2), "--out-dir", str(ws2 / "exports" / "moved_exp")]), 0)
            self.assertTrue((ws2 / "exports" / "moved_exp" / "artifact_inventory.json").is_file())

    def test_m023_r02_completeness_trust_boundary_and_jats_empty_body(self):
        # Imported completeness_verified=True WITHOUT completeness_verification_evidence cannot promote
        p_fake = {
            "paper_id": "fake_v",
            "Title": "Caller Claimed Complete",
            "full_text": "[page 1]\nSection 1: Intro\nText.",
            "completeness_verified": True,
        }
        ev_fake = acquire_evidence(p_fake)
        self.assertEqual(ev_fake["document_completeness"], "DECLARED_COMPLETE_UNVERIFIED")

        # Explicit completeness_verification_evidence + structured text yields VERIFIED_COMPLETE_PUBLICATION
        p_ok = dict(p_fake, completeness_verification_evidence={
            "status": "VERIFIED",
            "method": "checksum_and_page_count",
            "verifier": "test_harness",
            "evidence_ref": "sha256:abc",
        })
        ev_ok = acquire_evidence(p_ok)
        self.assertEqual(ev_ok["document_completeness"], "VERIFIED_COMPLETE_PUBLICATION")

        # JATS XML with empty <body> must not be promoted to full text
        with tempfile.TemporaryDirectory() as tmp:
            jats = Path(tmp) / "no_body.xml"
            jats.write_text("<article><front><article-meta><title-group><article-title>Only Abstract</article-title></title-group><abstract><p>Abs.</p></abstract></article-meta></front><body/></article>", encoding="utf-8")
            papers, _ = ingest_local_evidence([jats])
            ev_j = acquire_evidence(papers[0])
            self.assertNotEqual(ev_j["availability_status"], "FULL_TEXT_AVAILABLE")

    def test_m023_r03_recovery_after_failure_and_incompatible_schema(self):
        with tempfile.TemporaryDirectory() as tmp:
            t = Path(tmp)
            ws = t / "ws_rec"
            self.assertEqual(cli_main(["init", "--topic", "T", "--objective", "O", "--workspace", str(ws)]), 0)
            src = ws / "evidence" / "obs.txt"
            src.write_text("Observation 1.", encoding="utf-8")
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws), "--source", str(src)]), 0)

            # Inject failure on a later run (malformed JSON) -> prior outputs preserved, stage marked FAILED
            bad = t / "bad.json"
            bad.write_text("{bad", encoding="utf-8")
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws), "--input", str(bad), "--force"]), 2)
            m = json.loads((ws / WORKSPACE_FILENAME).read_text(encoding="utf-8"))
            self.assertEqual(m["stages"]["evidence"]["status"], "FAILED")
            self.assertTrue((ws / "runs" / "evidence" / "sota_report.md").is_file())

            # Successful deterministic stage restart via --resume with unchanged valid input
            self.assertEqual(cli_main(["evidence", "--workspace", str(ws), "--source", str(src), "--resume"]), 0)
            m2 = json.loads((ws / WORKSPACE_FILENAME).read_text(encoding="utf-8"))
            self.assertEqual(m2["stages"]["evidence"]["status"], "COMPLETED")
            self.assertEqual(m2["stages"]["evidence"]["recovery_mode"], "deterministic_stage_restart")

            # Incompatible workspace schema version refused
            m2["schema_version"] = "tako-workspace/v999"
            (ws / WORKSPACE_FILENAME).write_text(json.dumps(m2), encoding="utf-8")
            self.assertEqual(cli_main(["status", "--workspace", str(ws)]), 2)

            # Parent-directory symlink rejected
            real_dir = t / "real_dir"
            real_dir.mkdir()
            (real_dir / "f.txt").write_text("hi", encoding="utf-8")
            sym_dir = t / "sym_dir"
            sym_dir.symlink_to(real_dir)
            with self.assertRaises(ValueError):
                ingest_local_evidence([sym_dir / "f.txt"], base_dir=t)


if __name__ == "__main__":
    unittest.main()
