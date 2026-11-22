"""Minimal extensible multi-source scientific discovery providers and declarative registry (M024C)."""

from __future__ import annotations

import json
import os
import re
import urllib.parse
import xml.etree.ElementTree as ET
from typing import Any

from tools.sota_discovery.sota import (
    HttpBudgetExhaustedError,
    HttpRequestTracker,
    RetrievalError,
    fetch_openalex,
    fetch_openalex_citations,
)

ACCESS_MODES = (
    "FREE_NO_KEY",
    "FREE_WITH_KEY",
    "FREE_WITH_QUOTA",
    "INSTITUTIONAL_LICENSE",
    "PAID_API",
    "OAUTH_PERSONAL_LIBRARY",
    "MANUAL_ASSISTED",
    "UNSUPPORTED",
    "UNKNOWN",
)

PROVIDER_REGISTRY: dict[str, dict[str, Any]] = {
    "openalex": {
        "provider_id": "openalex",
        "display_name": "OpenAlex",
        "access_modes": ["FREE_NO_KEY", "FREE_WITH_KEY", "FREE_WITH_QUOTA", "PAID_API"],
        "implementation_status": "IMPLEMENTED",
        "qualification_status": "LIVE_QUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "citation_expansion", "open_access_location", "metadata_normalization"],
        "auth_env_vars": ["OPENALEX_API_KEY"],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid", "paid"],
        "default_enabled": True,
        "rate_limit_note": "Keyless: $0.10/day (10 rps); Free key: $1.00/day (30 rps); mailto deprecated.",
        "official_docs_url": "https://docs.openalex.org/",
        "verified_date": "2026-11-22",
    },
    "arxiv": {
        "provider_id": "arxiv",
        "display_name": "arXiv",
        "access_modes": ["FREE_NO_KEY", "FREE_WITH_QUOTA"],
        "implementation_status": "IMPLEMENTED",
        "qualification_status": "FIXTURE_QUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "open_access_location", "metadata_normalization"],
        "auth_env_vars": [],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid", "paid"],
        "default_enabled": False,
        "rate_limit_note": "No key; max 1 req / 3s per connection; Atom 1.0 XML; CC0 metadata.",
        "official_docs_url": "https://info.arxiv.org/help/api/index.html",
        "verified_date": "2026-11-22",
    },
    "crossref": {
        "provider_id": "crossref",
        "display_name": "Crossref",
        "access_modes": ["FREE_NO_KEY", "FREE_WITH_QUOTA"],
        "implementation_status": "IMPLEMENTED",
        "qualification_status": "FIXTURE_QUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "metadata_normalization"],
        "auth_env_vars": ["CROSSREF_MAILTO"],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid", "paid"],
        "default_enabled": False,
        "rate_limit_note": "Polite pool (User-Agent/mailto): 10 rps single DOI, 3 rps list; Public: 5/1 rps.",
        "official_docs_url": "https://api.crossref.org/swagger-ui/index.html",
        "verified_date": "2026-11-22",
    },
    "semantic-scholar": {
        "provider_id": "semantic-scholar",
        "display_name": "Semantic Scholar",
        "access_modes": ["FREE_NO_KEY", "FREE_WITH_KEY", "FREE_WITH_QUOTA"],
        "implementation_status": "IMPLEMENTED",
        "qualification_status": "FIXTURE_QUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "citation_expansion", "open_access_location", "metadata_normalization"],
        "auth_env_vars": ["SEMANTIC_SCHOLAR_API_KEY"],
        "key_required": False,
        "profiles": ["free", "hybrid", "paid"],
        "default_enabled": False,
        "rate_limit_note": "Anonymous shared pool (throttled); API key (x-api-key): 1 rps baseline.",
        "official_docs_url": "https://api.semanticscholar.org/api-docs/",
        "verified_date": "2026-11-22",
    },
    "google-scholar-assisted": {
        "provider_id": "google-scholar-assisted",
        "display_name": "Google Scholar (Assisted Search + Import)",
        "access_modes": ["MANUAL_ASSISTED", "UNSUPPORTED"],
        "implementation_status": "IMPLEMENTED",
        "qualification_status": "FIXTURE_QUALIFIED",
        "supported_capabilities": ["manual_query_generation", "manual_export_import"],
        "auth_env_vars": [],
        "key_required": False,
        "profiles": ["free", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "No official public API; automated scraping prohibited by ToS. Assisted URL/import only.",
        "official_docs_url": "https://scholar.google.com/intl/en/scholar/help.html",
        "verified_date": "2026-11-22",
    },
    "mendeley": {
        "provider_id": "mendeley",
        "display_name": "Mendeley",
        "access_modes": ["OAUTH_PERSONAL_LIBRARY", "FREE_WITH_KEY"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup"],
        "auth_env_vars": ["MENDELEY_CLIENT_ID", "MENDELEY_CLIENT_SECRET"],
        "key_required": True,
        "profiles": ["hybrid", "institutional"],
        "default_enabled": False,
        "rate_limit_note": "Requires OAuth 2.0 (client_credentials for catalog; auth code for personal library).",
        "official_docs_url": "https://dev.mendeley.com/",
        "verified_date": "2026-11-22",
    },
    "scopus": {
        "provider_id": "scopus",
        "display_name": "Scopus (Elsevier)",
        "access_modes": ["INSTITUTIONAL_LICENSE", "FREE_WITH_KEY", "PAID_API"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "citation_expansion"],
        "auth_env_vars": ["ELSEVIER_API_KEY", "ELSEVIER_INST_TOKEN"],
        "key_required": True,
        "profiles": ["institutional", "hybrid", "paid"],
        "default_enabled": False,
        "rate_limit_note": "Rolling 7-day quota; full metadata requires institutional IP or X-ELS-Insttoken.",
        "official_docs_url": "https://dev.elsevier.com/",
        "verified_date": "2026-11-22",
    },
    "ieee-xplore": {
        "provider_id": "ieee-xplore",
        "display_name": "IEEE Xplore",
        "access_modes": ["FREE_WITH_KEY", "INSTITUTIONAL_LICENSE"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "open_access_location"],
        "auth_env_vars": ["IEEE_API_KEY"],
        "key_required": True,
        "profiles": ["institutional", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "Requires manually approved developer portal key; 200 results/call, 25 DOIs/batch.",
        "official_docs_url": "https://developer.ieee.org/",
        "verified_date": "2026-11-22",
    },
    "web-of-science": {
        "provider_id": "web-of-science",
        "display_name": "Web of Science (Clarivate)",
        "access_modes": ["INSTITUTIONAL_LICENSE", "FREE_WITH_QUOTA", "PAID_API"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "citation_expansion"],
        "auth_env_vars": ["WOS_API_KEY"],
        "key_required": True,
        "profiles": ["institutional", "paid"],
        "default_enabled": False,
        "rate_limit_note": "Starter trial ~50 req/day; Expanded API requires institutional Clarivate license.",
        "official_docs_url": "https://developer.clarivate.com/",
        "verified_date": "2026-11-22",
    },
    "pubmed": {
        "provider_id": "pubmed",
        "display_name": "PubMed (NCBI E-utilities)",
        "access_modes": ["FREE_NO_KEY", "FREE_WITH_KEY"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup"],
        "auth_env_vars": ["NCBI_API_KEY"],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "3 rps keyless; 10 rps with NCBI api_key.",
        "official_docs_url": "https://www.ncbi.nlm.nih.gov/books/NBK25497/",
        "verified_date": "2026-11-22",
    },
    "europe-pmc": {
        "provider_id": "europe-pmc",
        "display_name": "Europe PMC",
        "access_modes": ["FREE_NO_KEY"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "citation_expansion", "open_access_location"],
        "auth_env_vars": [],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "No key required; polite throttling recommended.",
        "official_docs_url": "https://europepmc.org/RestfulWebService",
        "verified_date": "2026-11-22",
    },
    "core": {
        "provider_id": "core",
        "display_name": "CORE",
        "access_modes": ["FREE_WITH_KEY", "FREE_WITH_QUOTA"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "open_access_location"],
        "auth_env_vars": ["CORE_API_KEY"],
        "key_required": True,
        "profiles": ["free", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "Free registration API key required; dynamic X-RateLimit headers.",
        "official_docs_url": "https://api.core.ac.uk/docs/v3",
        "verified_date": "2026-11-22",
    },
    "unpaywall": {
        "provider_id": "unpaywall",
        "display_name": "Unpaywall",
        "access_modes": ["FREE_NO_KEY", "FREE_WITH_QUOTA"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["identifier_lookup", "open_access_location"],
        "auth_env_vars": ["UNPAYWALL_EMAIL"],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "Email param required; 100,000 calls/day guideline; DOI lookup only.",
        "official_docs_url": "https://unpaywall.org/products/api",
        "verified_date": "2026-11-22",
    },
    "zotero": {
        "provider_id": "zotero",
        "display_name": "Zotero (Web & Local API)",
        "access_modes": ["OAUTH_PERSONAL_LIBRARY", "FREE_WITH_KEY"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup"],
        "auth_env_vars": ["ZOTERO_API_KEY", "ZOTERO_USER_ID"],
        "key_required": True,
        "profiles": ["hybrid", "offline"],
        "default_enabled": False,
        "rate_limit_note": "Personal library; Backoff/Retry-After headers on Web API; unlimited on localhost:23119.",
        "official_docs_url": "https://www.zotero.org/support/dev/web_api/v3/start",
        "verified_date": "2026-11-22",
    },
    "sciencedirect": {
        "provider_id": "sciencedirect",
        "display_name": "ScienceDirect (Elsevier)",
        "access_modes": ["INSTITUTIONAL_LICENSE", "PAID_API"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "open_access_location"],
        "auth_env_vars": ["ELSEVIER_API_KEY", "ELSEVIER_INST_TOKEN"],
        "key_required": True,
        "profiles": ["institutional", "paid"],
        "default_enabled": False,
        "rate_limit_note": "Institutional TDM/API entitlement required for full-text retrieval.",
        "official_docs_url": "https://dev.elsevier.com/sciencedirect.html",
        "verified_date": "2026-11-22",
    },
    "springer-nature": {
        "provider_id": "springer-nature",
        "display_name": "Springer Nature",
        "access_modes": ["FREE_WITH_KEY", "FREE_WITH_QUOTA", "INSTITUTIONAL_LICENSE", "PAID_API"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "open_access_location"],
        "auth_env_vars": ["SPRINGER_API_KEY"],
        "key_required": True,
        "profiles": ["free", "institutional", "paid"],
        "default_enabled": False,
        "rate_limit_note": "API key required; rolling 24h daily quota by Basic vs Premium tier.",
        "official_docs_url": "https://dev.springernature.com/",
        "verified_date": "2026-11-22",
    },
    "dblp": {
        "provider_id": "dblp",
        "display_name": "DBLP",
        "access_modes": ["FREE_NO_KEY"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup"],
        "auth_env_vars": [],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "No key; 1-2s polite delay recommended; HTTP 429 with Retry-After.",
        "official_docs_url": "https://dblp.org/faq/13501473.html",
        "verified_date": "2026-11-22",
    },
    "openaire": {
        "provider_id": "openaire",
        "display_name": "OpenAIRE Graph",
        "access_modes": ["FREE_NO_KEY", "FREE_WITH_KEY", "FREE_WITH_QUOTA"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup", "open_access_location"],
        "auth_env_vars": ["OPENAIRE_ACCESS_TOKEN"],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "60 req/hr unauthenticated; 7,200 req/hr with OAuth 2.0 Bearer token.",
        "official_docs_url": "https://graph.openaire.eu/docs/",
        "verified_date": "2026-11-22",
    },
    "oai-pmh": {
        "provider_id": "oai-pmh",
        "display_name": "Institutional Repositories (OAI-PMH)",
        "access_modes": ["FREE_NO_KEY", "INSTITUTIONAL_LICENSE"],
        "implementation_status": "DOCUMENTED_ONLY",
        "qualification_status": "CURRENTLY_UNQUALIFIED",
        "supported_capabilities": ["search", "identifier_lookup"],
        "auth_env_vars": [],
        "key_required": False,
        "profiles": ["free", "institutional", "hybrid"],
        "default_enabled": False,
        "rate_limit_note": "Harvesting protocol using resumptionTokens; per-repository rate policies.",
        "official_docs_url": "https://www.openarchives.org/pmh/",
        "verified_date": "2026-11-22",
    },
    "fixture": {
        "provider_id": "fixture",
        "display_name": "Offline JSON Fixture",
        "access_modes": ["FREE_NO_KEY"],
        "implementation_status": "IMPLEMENTED",
        "qualification_status": "OFFLINE_QUALIFIED",
        "supported_capabilities": ["search"],
        "auth_env_vars": [],
        "key_required": False,
        "profiles": ["offline", "free"],
        "default_enabled": False,
        "rate_limit_note": "Zero network calls; deterministic local testing only.",
        "official_docs_url": "",
        "verified_date": "2026-11-22",
    },
}


class UnsupportedProviderOperationError(RuntimeError):
    """Raised when a provider is asked to perform an unsupported capability."""


def extract_arxiv_id(raw: str) -> str:
    """Normalize an arXiv identifier (stripping URL prefixes and version suffixes)."""
    text = str(raw or "").strip()
    if not text:
        return ""
    low = text.lower()
    m = re.search(r"10\.48550/arxiv\.([0-9]{4}\.[0-9]{4,5}(?:v\d+)?|[a-z\-]+/[0-9]{7}(?:v\d+)?)", low)
    if m:
        return re.sub(r"v\d+$", "", m.group(1))
    m2 = re.search(r"(?:arxiv\.org/(?:abs|pdf)/|arxiv:)([0-9]{4}\.[0-9]{4,5}(?:v\d+)?|[a-z\-]+/[0-9]{7}(?:v\d+)?)", low)
    if m2:
        return re.sub(r"v\d+$", "", m2.group(1))
    m3 = re.fullmatch(r"([0-9]{4}\.[0-9]{4,5}(?:v\d+)?)", low)
    if m3:
        return re.sub(r"v\d+$", "", m3.group(1))
    return ""


def _invert_abstract(text: str) -> dict[str, list[int]] | None:
    words = [w for w in str(text or "").split() if w]
    if not words:
        return None
    inv: dict[str, list[int]] = {}
    for idx, w in enumerate(words):
        inv.setdefault(w, []).append(idx)
    return inv


def list_provider_capabilities(profile: str | None = None) -> list[dict[str, Any]]:
    """Return declarative provider capability rows with boolean credential presence (never secret values)."""
    rows: list[dict[str, Any]] = []
    for pid, spec in PROVIDER_REGISTRY.items():
        if profile and profile not in spec["profiles"]:
            continue
        env_vars = spec["auth_env_vars"]
        env_status = {var: bool(os.environ.get(var, "").strip()) for var in env_vars}
        configured = True if not spec["key_required"] else any(env_status.values())
        rows.append({
            **spec,
            "credentials_present": env_status,
            "configured": configured,
        })
    return rows


class DiscoveryProvider:
    """Minimal shared contract for scientific discovery providers."""

    provider_id: str = "base"

    def search(self, query: str, max_results: int, *, tracker: HttpRequestTracker) -> list[dict[str, Any]]:
        raise UnsupportedProviderOperationError(f"{self.provider_id} does not support search.")

    def lookup_identifier(self, identifier: str, *, tracker: HttpRequestTracker) -> dict[str, Any] | None:
        raise UnsupportedProviderOperationError(f"{self.provider_id} does not support identifier_lookup.")

    def expand_citations(
        self,
        work: dict[str, Any],
        *,
        max_per_direction: int = 5,
        tracker: HttpRequestTracker,
    ) -> dict[str, list[dict[str, Any]]]:
        raise UnsupportedProviderOperationError(f"{self.provider_id} does not support citation_expansion.")


class OpenAlexProvider(DiscoveryProvider):
    provider_id = "openalex"

    def search(self, query: str, max_results: int, *, tracker: HttpRequestTracker) -> list[dict[str, Any]]:
        works = fetch_openalex(query, per_page=max_results, tracker=tracker)
        for w in works:
            w.setdefault("_provider_source", self.provider_id)
            w.setdefault("publication_stage", "preprint" if extract_arxiv_id(str(w.get("doi") or "")) else "published")
        return works

    def lookup_identifier(self, identifier: str, *, tracker: HttpRequestTracker) -> dict[str, Any] | None:
        clean = str(identifier or "").strip()
        if not clean:
            return None
        url = f"https://api.openalex.org/works/{urllib.parse.quote(clean, safe=':/')}"
        try:
            payload = tracker.get_json(url, timeout=15.0, category="bib_verification", provider=self.provider_id)
        except RetrievalError:
            return None
        if isinstance(payload, dict) and payload.get("id"):
            payload.setdefault("_provider_source", self.provider_id)
            return payload
        return None

    def expand_citations(
        self,
        work: dict[str, Any],
        *,
        max_per_direction: int = 5,
        tracker: HttpRequestTracker,
    ) -> dict[str, list[dict[str, Any]]]:
        out = fetch_openalex_citations(work, max_per_direction=max_per_direction, tracker=tracker)
        for direction in ("backward", "forward"):
            for w in out.get(direction) or []:
                w.setdefault("_provider_source", self.provider_id)
        return out


class ArxivProvider(DiscoveryProvider):
    provider_id = "arxiv"
    API_URL = "https://export.arxiv.org/api/query"
    NS = {
        "atom": "http://www.w3.org/2005/Atom",
        "arxiv": "http://arxiv.org/schemas/atom",
    }

    def _parse_atom(self, xml_text: str) -> list[dict[str, Any]]:
        root = ET.fromstring(xml_text)
        works: list[dict[str, Any]] = []
        for entry in root.findall("atom:entry", self.NS):
            raw_id = (entry.findtext("atom:id", default="", namespaces=self.NS) or "").strip()
            arxiv_id = extract_arxiv_id(raw_id)
            title = " ".join((entry.findtext("atom:title", default="", namespaces=self.NS) or "").split())
            if not title or title.lower() == "error":
                continue
            summary = " ".join((entry.findtext("atom:summary", default="", namespaces=self.NS) or "").split())
            published = (entry.findtext("atom:published", default="", namespaces=self.NS) or "").strip()
            year = int(published[:4]) if len(published) >= 4 and published[:4].isdigit() else None
            authors = [
                {"author": {"display_name": " ".join((a.findtext("atom:name", default="", namespaces=self.NS) or "").split())}}
                for a in entry.findall("atom:author", self.NS)
                if (a.findtext("atom:name", default="", namespaces=self.NS) or "").strip()
            ]
            doi_el = entry.find("arxiv:doi", self.NS)
            doi_text = (doi_el.text or "").strip() if doi_el is not None and doi_el.text else ""
            doi_url = f"https://doi.org/{doi_text}" if doi_text and not doi_text.startswith("http") else doi_text
            pdf_url = ""
            for link in entry.findall("atom:link", self.NS):
                if link.attrib.get("title") == "pdf" or link.attrib.get("type") == "application/pdf":
                    pdf_url = link.attrib.get("href", "").strip()
                    break
            cats = [
                {"display_name": c.attrib.get("term", "").strip()}
                for c in entry.findall("atom:category", self.NS)
                if c.attrib.get("term")
            ]
            works.append({
                "id": f"https://arxiv.org/abs/{arxiv_id}" if arxiv_id else raw_id,
                "arxiv_id": arxiv_id,
                "doi": doi_url or (f"https://doi.org/10.48550/arXiv.{arxiv_id}" if arxiv_id else ""),
                "display_name": title,
                "publication_year": year,
                "cited_by_count": 0,
                "authorships": authors,
                "primary_location": {
                    "source": {"display_name": "arXiv"},
                    "landing_page_url": raw_id or (f"https://arxiv.org/abs/{arxiv_id}" if arxiv_id else ""),
                    "pdf_url": pdf_url,
                },
                "open_access": {"is_oa": True, "oa_url": pdf_url or raw_id},
                "concepts": cats,
                "abstract": summary,
                "abstract_inverted_index": _invert_abstract(summary),
                "type": "preprint",
                "publication_stage": "preprint",
                "_provider_source": self.provider_id,
            })
        return works

    def search(self, query: str, max_results: int, *, tracker: HttpRequestTracker) -> list[dict[str, Any]]:
        params = urllib.parse.urlencode({
            "search_query": f"all:{query}",
            "start": "0",
            "max_results": str(max(1, min(max_results, 25))),
        })
        xml_text = tracker.get_text(f"{self.API_URL}?{params}", timeout=15.0, category="search", provider=self.provider_id)
        return self._parse_atom(xml_text)

    def lookup_identifier(self, identifier: str, *, tracker: HttpRequestTracker) -> dict[str, Any] | None:
        aid = extract_arxiv_id(identifier)
        if not aid:
            return None
        params = urllib.parse.urlencode({"id_list": aid})
        xml_text = tracker.get_text(f"{self.API_URL}?{params}", timeout=15.0, category="bib_verification", provider=self.provider_id)
        hits = self._parse_atom(xml_text)
        return hits[0] if hits else None


class CrossrefProvider(DiscoveryProvider):
    provider_id = "crossref"
    API_URL = "https://api.crossref.org/works"

    def _headers(self) -> dict[str, str]:
        mailto = os.environ.get("CROSSREF_MAILTO", "").strip()
        ua = f"TAKO-Discovery/0.1 (mailto:{mailto})" if mailto else "TAKO-Discovery/0.1 (mailto:research-local@example.org)"
        return {"User-Agent": ua}

    def _normalize_item(self, item: dict[str, Any]) -> dict[str, Any]:
        doi = str(item.get("DOI") or "").strip()
        doi_url = f"https://doi.org/{doi}" if doi else ""
        titles = item.get("title") or []
        title = str(titles[0] if titles else "").strip() or "UNKNOWN"
        authors = []
        for a in item.get("author") or []:
            name = " ".join(p for p in [str(a.get("given") or "").strip(), str(a.get("family") or "").strip()] if p)
            if name:
                authors.append({"author": {"display_name": name}})
        year = None
        for date_key in ("published-print", "published-online", "issued", "created"):
            parts = ((item.get(date_key) or {}).get("date-parts") or [[]])[0]
            if parts and isinstance(parts[0], int):
                year = parts[0]
                break
        containers = item.get("container-title") or []
        venue = str(containers[0] if containers else "").strip() or "UNKNOWN"
        abstract = re.sub(r"<[^>]+>", " ", str(item.get("abstract") or "")).strip()
        abstract = " ".join(abstract.split())
        arxiv_id = extract_arxiv_id(doi)
        stage = "preprint" if (arxiv_id or str(item.get("type") or "").lower() == "posted-content") else "published"
        return {
            "id": doi_url or str(item.get("URL") or ""),
            "doi": doi_url,
            "arxiv_id": arxiv_id,
            "display_name": title,
            "publication_year": year,
            "cited_by_count": int(item.get("is-referenced-by-count") or 0),
            "authorships": authors,
            "primary_location": {
                "source": {"display_name": venue},
                "landing_page_url": str(item.get("URL") or doi_url),
            },
            "concepts": [{"display_name": s} for s in (item.get("subject") or []) if s],
            "abstract": abstract,
            "abstract_inverted_index": _invert_abstract(abstract),
            "type": str(item.get("type") or "journal-article").lower(),
            "publication_stage": stage,
            "_provider_source": self.provider_id,
        }

    def search(self, query: str, max_results: int, *, tracker: HttpRequestTracker) -> list[dict[str, Any]]:
        params = urllib.parse.urlencode({"query": query, "rows": str(max(1, min(max_results, 25)))})
        payload = tracker.get_json(f"{self.API_URL}?{params}", timeout=15.0, category="search", provider=self.provider_id, headers=self._headers())
        items = ((payload.get("message") or {}).get("items") or []) if isinstance(payload, dict) else []
        return [self._normalize_item(it) for it in items if isinstance(it, dict)]

    def lookup_identifier(self, identifier: str, *, tracker: HttpRequestTracker) -> dict[str, Any] | None:
        clean = re.sub(r"^https?://(?:dx\.)?doi\.org/", "", str(identifier or "").strip(), flags=re.IGNORECASE)
        if not clean.startswith("10."):
            return None
        url = f"{self.API_URL}/{urllib.parse.quote(clean, safe='/')}"
        try:
            payload = tracker.get_json(url, timeout=15.0, category="bib_verification", provider=self.provider_id, headers=self._headers())
        except RetrievalError:
            return None
        msg = payload.get("message") if isinstance(payload, dict) else None
        return self._normalize_item(msg) if isinstance(msg, dict) else None


class SemanticScholarProvider(DiscoveryProvider):
    provider_id = "semantic-scholar"
    API_URL = "https://api.semanticscholar.org/graph/v1"
    FIELDS = "paperId,externalIds,title,abstract,year,citationCount,authors,venue,isOpenAccess,openAccessPdf,fieldsOfStudy"

    def _headers(self) -> dict[str, str]:
        key = os.environ.get("SEMANTIC_SCHOLAR_API_KEY", "").strip()
        return {"x-api-key": key} if key else {}

    def _normalize_item(self, item: dict[str, Any]) -> dict[str, Any]:
        ext = item.get("externalIds") or {}
        doi = str(ext.get("DOI") or "").strip()
        doi_url = f"https://doi.org/{doi}" if doi else ""
        arxiv_id = extract_arxiv_id(str(ext.get("ArXiv") or doi))
        pid = str(item.get("paperId") or "").strip()
        oa_pdf = ((item.get("openAccessPdf") or {}).get("url") or "").strip()
        abstract = str(item.get("abstract") or "").strip()
        return {
            "id": f"https://www.semanticscholar.org/paper/{pid}" if pid else doi_url,
            "s2_id": pid,
            "doi": doi_url,
            "arxiv_id": arxiv_id,
            "display_name": str(item.get("title") or "UNKNOWN").strip(),
            "publication_year": item.get("year"),
            "cited_by_count": int(item.get("citationCount") or 0),
            "authorships": [{"author": {"display_name": a.get("name", "")}} for a in (item.get("authors") or []) if a.get("name")],
            "primary_location": {
                "source": {"display_name": str(item.get("venue") or "").strip() or ("arXiv" if arxiv_id else "UNKNOWN")},
                "landing_page_url": doi_url or (f"https://www.semanticscholar.org/paper/{pid}" if pid else ""),
                "pdf_url": oa_pdf,
            },
            "open_access": {"is_oa": bool(item.get("isOpenAccess") or oa_pdf), "oa_url": oa_pdf},
            "concepts": [{"display_name": f} for f in (item.get("fieldsOfStudy") or []) if f],
            "abstract": abstract,
            "abstract_inverted_index": _invert_abstract(abstract),
            "type": "preprint" if (arxiv_id and not doi) else "article",
            "publication_stage": "preprint" if (arxiv_id and not doi) else "published",
            "_provider_source": self.provider_id,
        }

    def search(self, query: str, max_results: int, *, tracker: HttpRequestTracker) -> list[dict[str, Any]]:
        params = urllib.parse.urlencode({"query": query, "limit": str(max(1, min(max_results, 25))), "fields": self.FIELDS})
        payload = tracker.get_json(f"{self.API_URL}/paper/search?{params}", timeout=15.0, category="search", provider=self.provider_id, headers=self._headers())
        data = payload.get("data") or [] if isinstance(payload, dict) else []
        return [self._normalize_item(it) for it in data if isinstance(it, dict)]

    def lookup_identifier(self, identifier: str, *, tracker: HttpRequestTracker) -> dict[str, Any] | None:
        clean = str(identifier or "").strip()
        if not clean:
            return None
        params = urllib.parse.urlencode({"fields": self.FIELDS})
        url = f"{self.API_URL}/paper/{urllib.parse.quote(clean, safe=':/')}?{params}"
        try:
            payload = tracker.get_json(url, timeout=15.0, category="bib_verification", provider=self.provider_id, headers=self._headers())
        except RetrievalError:
            return None
        return self._normalize_item(payload) if isinstance(payload, dict) and payload.get("paperId") else None


class GoogleScholarAssistedProvider(DiscoveryProvider):
    """Manual assisted search URL generator + BibTeX/JSON import (zero automated scraping)."""

    provider_id = "google-scholar-assisted"

    def build_assisted_search_plan(self, queries: list[str]) -> dict[str, Any]:
        links = [
            {
                "query": q,
                "search_url": f"https://scholar.google.com/scholar?{urllib.parse.urlencode({'q': q})}",
                "instructions": "Open manually in browser, export relevant BibTeX/RIS entries, and import via TAKO.",
            }
            for q in queries
            if str(q).strip()
        ]
        return {
            "provider": self.provider_id,
            "automation_allowed": False,
            "reason": "Google Scholar provides no official public API and prohibits automated scraping.",
            "assisted_links": links,
        }

    def import_json_records(self, records: list[dict[str, Any]]) -> list[dict[str, Any]]:
        out: list[dict[str, Any]] = []
        for r in records:
            out.append({
                "id": str(r.get("url") or r.get("doi") or f"gs-assisted:{len(out)+1}"),
                "doi": str(r.get("doi") or ""),
                "arxiv_id": extract_arxiv_id(str(r.get("url") or r.get("doi") or "")),
                "display_name": str(r.get("title") or "UNKNOWN").strip(),
                "publication_year": r.get("year"),
                "cited_by_count": int(r.get("cited_by") or 0),
                "authorships": [{"author": {"display_name": a}} for a in (r.get("authors") or [])],
                "primary_location": {"source": {"display_name": str(r.get("venue") or "UNKNOWN")}},
                "concepts": [],
                "abstract": str(r.get("abstract") or ""),
                "abstract_inverted_index": _invert_abstract(str(r.get("abstract") or "")),
                "type": "article",
                "publication_stage": "UNKNOWN",
                "_provider_source": "google-scholar-assisted-import",
            })
        return out


_IMPLEMENTED_FACTORIES: dict[str, type[DiscoveryProvider]] = {
    "openalex": OpenAlexProvider,
    "arxiv": ArxivProvider,
    "crossref": CrossrefProvider,
    "semantic-scholar": SemanticScholarProvider,
    "google-scholar-assisted": GoogleScholarAssistedProvider,
}


def get_provider(provider_id: str) -> DiscoveryProvider:
    pid = str(provider_id or "").strip().lower()
    if pid in _IMPLEMENTED_FACTORIES:
        return _IMPLEMENTED_FACTORIES[pid]()
    if pid in PROVIDER_REGISTRY:
        raise UnsupportedProviderOperationError(
            f"Provider '{pid}' is documented ({PROVIDER_REGISTRY[pid]['access_modes']}) but not implemented in 0.0.1.dev9."
        )
    raise ValueError(f"Unknown discovery provider: {provider_id!r}")


def run_multi_source_search(
    query: str,
    max_results_per_source: int,
    providers: list[DiscoveryProvider],
    *,
    tracker: HttpRequestTracker,
) -> tuple[list[dict[str, Any]], list[str]]:
    """Query multiple providers under a shared HttpRequestTracker, isolating per-provider failures."""
    combined: list[dict[str, Any]] = []
    errors: list[str] = []
    for prov in providers:
        try:
            hits = prov.search(query, max_results_per_source, tracker=tracker)
            for h in hits:
                h.setdefault("_provider_source", prov.provider_id)
            combined.extend(hits)
        except HttpBudgetExhaustedError:
            raise
        except (RetrievalError, UnsupportedProviderOperationError, Exception) as exc:
            errors.append(f"{prov.provider_id}:{exc}")
    return combined, errors
