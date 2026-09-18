#!/usr/bin/env python3
"""Reject release publication unless GitHub verifies the annotated GPG tag."""

import json
import os
import subprocess
import sys
from urllib.parse import quote


def github_api(path):
    return json.loads(subprocess.check_output(["gh", "api", path], text=True))


def verify_tag(repository, tag_name, commit_sha, api=github_api):
    prefix = f"repos/{repository}/git"
    ref = api(f"{prefix}/ref/tags/{quote(tag_name, safe='')}")
    if ref.get("ref") != f"refs/tags/{tag_name}":
        raise ValueError("GitHub returned a different tag ref")
    obj = ref.get("object", {})
    if obj.get("type") != "tag":
        raise ValueError("Release tags must be annotated; lightweight tags are rejected")
    tag = api(f"{prefix}/tags/{obj['sha']}")
    if tag.get("sha") != obj["sha"] or tag.get("tag") != tag_name:
        raise ValueError("Tag object does not match the requested release tag")
    target = tag.get("object", {})
    if target.get("type") != "commit" or target.get("sha") != commit_sha:
        raise ValueError("Release tag must directly identify this workflow's commit")
    verification = tag.get("verification", {})
    if verification.get("verified") is not True or verification.get("reason") != "valid":
        raise ValueError("GitHub has not verified the release tag signature")
    signature = verification.get("signature") or ""
    if not signature.startswith("-----BEGIN PGP SIGNATURE-----"):
        raise ValueError("Release tags must carry a GPG signature")


def main():
    if os.environ["GITHUB_REF_TYPE"] != "tag":
        print("Branch build: release-tag verification does not apply")
        return
    verify_tag(os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_REF_NAME"], os.environ["GITHUB_SHA"])
    print("Release tag has a verified GPG signature and matches the build commit")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Release tag verification failed: {error}", file=sys.stderr)
        sys.exit(1)
