#!/usr/bin/env python3
"""
scrape_aws_docs.py — fetch markdown for the 18-service AWS docs slice.

Pass 1: sitemap-driven. For each service, find all per-guide sitemaps,
        get all .html URLs, swap to .md, fetch, save to disk.
Pass 2: link-driven. Parse saved .md files for relative .md links,
        fetch anything not on disk. Repeat until no new files appear.

Output: a `corpus/` tree mirroring docs.aws.amazon.com's folder structure.
Polite (0.5s delay), resumable (skips files already on disk).

Logs (created in current working directory, append mode):
    scraper.log     — full run log, replayable
    failures.jsonl  — one JSON record per failed URL, greppable with jq

Usage:
    pip install requests
    python scrape_aws_docs.py
    # or limit to a few services for testing:
    python scrape_aws_docs.py --services ec2 s3

Inspecting failures later:
    cat failures.jsonl | jq .
    cat failures.jsonl | jq -r '.url'
"""

import argparse
import json
import logging
import re
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urljoin, urlparse

import requests


BASE = "https://docs.aws.amazon.com"
USER_AGENT = "krithik-search-engine-scraper/0.1 (educational; github.com/sreenish27)"
DELAY = 0.5
MAX_RETRIES = 4

FAILURES_LOG = "failures.jsonl"
RUN_LOG = "scraper.log"

# 18 services. Each entry lists the docs path-slugs AWS uses in URLs.
# Some services have multiple slugs (e.g. apigateway has both v1 and v2).
SERVICES = {
    "ec2":            ["AWSEC2"],
    "s3":             ["AmazonS3"],
    "iam":            ["IAM"],
    "lambda":         ["lambda"],
    "vpc":            ["vpc"],
    "rds":            ["AmazonRDS"],
    "cloudwatch":     ["AmazonCloudWatch"],
    "dynamodb":       ["amazondynamodb"],
    "cloudformation": ["AWSCloudFormation"],
    "apigateway":     ["apigateway", "apigatewayv2"],
    "route53":        ["Route53"],
    "sqs":            ["AWSSimpleQueueService"],
    "sns":            ["sns"],
    "ecs":            ["AmazonECS"],
    "eks":            ["eks"],
    "cloudfront":     ["AmazonCloudFront"],
    "cognito":        ["cognito", "cognito-user-identity-pools"],
    "kms":            ["kms"],
}

# Common guide directory names AWS uses across services.
# We try each per slug; sitemap fetch returns 404 means that guide doesn't exist.
GUIDE_CANDIDATES = [
    "UserGuide", "userguide",
    "APIReference", "APIreference", "apireference", "API",
    "DeveloperGuide", "developerguide", "dg",
    "CommandLineReference",
    "monitoring",
    "operatorguide",
    "bestpractices",
    "SQSDeveloperGuide",
]


logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s | %(levelname)s | %(message)s",
    handlers=[
        logging.FileHandler(RUN_LOG, mode="a", encoding="utf-8"),
        logging.StreamHandler(sys.stdout),
    ],
)
log = logging.getLogger("scraper")


def log_failure(url, pass_name, reason, iteration=None):
    """Append one JSON record per failed URL to failures.jsonl."""
    record = {
        "ts": datetime.now(timezone.utc).isoformat(),
        "url": url,
        "pass": pass_name,
        "iteration": iteration,
        "reason": reason,
    }
    with open(FAILURES_LOG, "a", encoding="utf-8") as f:
        f.write(json.dumps(record) + "\n")


class Session:
    def __init__(self):
        self.s = requests.Session()
        self.s.headers.update({"User-Agent": USER_AGENT})
        self._last = 0.0
        # set by get() so callers can record *why* a URL failed.
        self.last_error = None

    def get(self, url):
        self.last_error = None
        delta = time.time() - self._last
        if delta < DELAY:
            time.sleep(DELAY - delta)
        for attempt in range(MAX_RETRIES):
            try:
                r = self.s.get(url, timeout=20)
                self._last = time.time()
                if r.status_code == 404:
                    self.last_error = "http_404"
                    return None
                if r.status_code == 429:
                    wait = 2 ** (attempt + 1)
                    log.warning(f"429 on {url}, backing off {wait}s")
                    time.sleep(wait)
                    continue
                r.raise_for_status()
                return r.text
            except requests.RequestException as e:
                self.last_error = f"{type(e).__name__}: {e}"
                wait = 2 ** attempt
                log.warning(f"error on {url}: {e}; retry in {wait}s")
                time.sleep(wait)
        log.error(f"gave up on {url}")
        if self.last_error is None:
            self.last_error = "max_retries_exhausted"
        return None


def url_to_path(out_dir, url):
    """https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/concepts.md
       -> out_dir/AWSEC2/latest/UserGuide/concepts.md"""
    rel = urlparse(url).path.lstrip("/")
    return out_dir / rel


def discover_sitemaps(sess, slugs):
    """For each slug, try every guide candidate; collect URLs from sitemaps that exist."""
    urls = set()
    for slug in slugs:
        for guide in GUIDE_CANDIDATES:
            sm_url = f"{BASE}/{slug}/latest/{guide}/sitemap.xml"
            xml = sess.get(sm_url)
            if xml is None:
                continue
            locs = re.findall(r"<loc>([^<]+)</loc>", xml)
            if locs:
                log.info(f"  {slug}/{guide}: {len(locs)} pages")
                urls.update(locs)
    return urls


def html_to_md_url(html_url):
    if html_url.endswith(".html"):
        return html_url[:-5] + ".md"
    return html_url + ".md"


def pass1_sitemap(sess, out_dir, services):
    """Fetch markdown for every URL in every sitemap of every service."""
    all_md_urls = set()
    log.info("=== PASS 1: sitemap discovery ===")
    for svc in services:
        slugs = SERVICES[svc]
        log.info(f"service: {svc} (slugs={slugs})")
        html_urls = discover_sitemaps(sess, slugs)
        md_urls = {html_to_md_url(u) for u in html_urls}
        log.info(f"  total unique pages: {len(md_urls)}")
        all_md_urls.update(md_urls)

    log.info(f"=== PASS 1: fetching {len(all_md_urls)} markdown files ===")
    fetched, skipped, failed = 0, 0, 0
    for i, url in enumerate(sorted(all_md_urls), 1):
        target = url_to_path(out_dir, url)
        if target.exists():
            skipped += 1
            continue
        if i % 100 == 0:
            log.info(f"  progress: {i}/{len(all_md_urls)} fetched={fetched} skipped={skipped} failed={failed}")
        md = sess.get(url)
        if md is None:
            failed += 1
            log_failure(url, pass_name="pass1", reason=sess.last_error or "unknown")
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(md, encoding="utf-8")
        fetched += 1
    log.info(f"PASS 1 done: fetched={fetched} skipped={skipped} failed={failed}")
    return fetched + skipped


def extract_md_links(md_text, current_md_url):
    """Find [text](path.md) and [text](path.md#anchor); resolve relative to current page."""
    pattern = r"\[[^\]]*\]\(([^)\s]+\.md(?:#[^)]*)?)\)"
    targets = set()
    for raw in re.findall(pattern, md_text):
        path_part = raw.split("#")[0]
        absolute = urljoin(current_md_url, path_part)
        if absolute.startswith(BASE):
            targets.add(absolute)
    return targets


def pass2_link_follow(sess, out_dir):
    """Read every saved .md, extract .md links, fetch missing ones. Loop until fixed point."""
    log.info("=== PASS 2: link-driven verification ===")
    iteration = 0
    while True:
        iteration += 1
        log.info(f"iteration {iteration}: scanning saved files for links...")
        targets = set()
        for f in out_dir.rglob("*.md"):
            try:
                txt = f.read_text(encoding="utf-8", errors="replace")
            except Exception:
                continue
            rel = f.relative_to(out_dir).as_posix()
            current_url = f"{BASE}/{rel}"
            targets.update(extract_md_links(txt, current_url))

        missing = []
        for url in targets:
            path = url_to_path(out_dir, url)
            if not path.exists():
                missing.append(url)
        log.info(f"  referenced: {len(targets)}  missing: {len(missing)}")
        if not missing:
            log.info(f"PASS 2: fixed point reached after {iteration} iteration(s)")
            return
        fetched, failed = 0, 0
        for i, url in enumerate(missing, 1):
            target = url_to_path(out_dir, url)
            md = sess.get(url)
            if md is None:
                failed += 1
                log_failure(url, pass_name="pass2", reason=sess.last_error or "unknown", iteration=iteration)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(md, encoding="utf-8")
            fetched += 1
            if i % 100 == 0:
                log.info(f"  fetching: {i}/{len(missing)} fetched={fetched} failed={failed}")
        log.info(f"iteration {iteration} done: fetched={fetched} failed={failed}")
        if fetched == 0:
            log.info("no new pages successfully fetched; stopping.")
            return


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="corpus", help="output directory (default: ./corpus)")
    ap.add_argument("--services", nargs="*", help="subset of services (default: all 18)")
    ap.add_argument("--skip-pass1", action="store_true", help="skip pass 1 (sitemap)")
    ap.add_argument("--skip-pass2", action="store_true", help="skip pass 2 (link follow)")
    args = ap.parse_args()

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    services = args.services or list(SERVICES.keys())
    bad = [s for s in services if s not in SERVICES]
    if bad:
        log.error(f"unknown services: {bad}")
        log.error(f"known: {list(SERVICES.keys())}")
        sys.exit(1)

    log.info(f"=== RUN START === services={services} out={out_dir.resolve()}")
    sess = Session()
    if not args.skip_pass1:
        pass1_sitemap(sess, out_dir, services)
    if not args.skip_pass2:
        pass2_link_follow(sess, out_dir)

    total = sum(1 for _ in out_dir.rglob("*.md"))
    log.info(f"=== DONE === total .md files on disk: {total}")
    log.info(f"corpus saved to: {out_dir.resolve()}")
    log.info(f"failures (if any) appended to: {Path(FAILURES_LOG).resolve()}")
    log.info(f"full run log appended to: {Path(RUN_LOG).resolve()}")


if __name__ == "__main__":
    main()