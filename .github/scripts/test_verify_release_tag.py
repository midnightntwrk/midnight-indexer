"""Exercise the publication boundary without publishing images or changing refs."""

import copy
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("verify_release_tag", Path(__file__).with_name("verify-release-tag.py"))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


class VerifyReleaseTagTest(unittest.TestCase):
    def setUp(self):
        self.ref = {"ref": "refs/tags/v1.2.3", "object": {"type": "tag", "sha": "a" * 40}}
        self.tag = {
            "sha": "a" * 40,
            "tag": "v1.2.3",
            "object": {"type": "commit", "sha": "b" * 40},
            "verification": {"verified": True, "reason": "valid", "signature": "-----BEGIN PGP SIGNATURE-----\nfixture"},
        }

    def verify(self):
        replies = iter([self.ref, self.tag])
        verifier.verify_tag("example/indexer", "v1.2.3", "b" * 40, api=lambda _: next(replies))

    def test_verified_annotated_tag(self):
        self.verify()

    def test_lightweight_tag(self):
        self.ref["object"]["type"] = "commit"
        with self.assertRaisesRegex(ValueError, "annotated"):
            self.verify()

    def test_unsigned_unverified_and_invalid_signatures(self):
        for reason in ["unsigned", "unknown_key", "invalid", "expired_key", "bad_email", "gpgverify_unavailable"]:
            with self.subTest(reason=reason):
                self.tag["verification"].update(verified=False, reason=reason)
                with self.assertRaisesRegex(ValueError, "not verified"):
                    self.verify()

    def test_inconsistent_verification(self):
        self.tag["verification"]["reason"] = "unsigned"
        with self.assertRaises(ValueError):
            self.verify()

    def test_missing_or_non_gpg_signature(self):
        for signature in [None, "", "-----BEGIN SSH SIGNATURE-----"]:
            with self.subTest(signature=signature):
                self.tag["verification"]["signature"] = signature
                with self.assertRaisesRegex(ValueError, "GPG"):
                    self.verify()

    def test_moved_nested_or_mismatched_tag(self):
        original = copy.deepcopy(self.tag)
        for field, value in [("object", {"type": "commit", "sha": "c" * 40}),
                             ("object", {"type": "tag", "sha": "b" * 40}),
                             ("tag", "v9.9.9"), ("sha", "d" * 40)]:
            with self.subTest(field=field, value=value):
                self.tag = copy.deepcopy(original)
                self.tag[field] = value
                with self.assertRaises(ValueError):
                    self.verify()

    def test_api_failure_does_not_pass(self):
        def failed_api(_):
            raise subprocess.CalledProcessError(1, "gh")
        with self.assertRaises(subprocess.CalledProcessError):
            verifier.verify_tag("example/indexer", "v1.2.3", "b" * 40, api=failed_api)

    def test_branch_build_does_not_query_tags(self):
        with patch.dict("os.environ", {"GITHUB_REF_TYPE": "branch"}), patch.object(verifier, "verify_tag") as check:
            verifier.main()
            check.assert_not_called()

    def test_tag_dispatch_cannot_skip_verification(self):
        env = {"GITHUB_REF_TYPE": "tag", "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REPOSITORY": "example/indexer",
               "GITHUB_REF_NAME": "v1.2.3", "GITHUB_SHA": "b" * 40}
        with patch.dict("os.environ", env), patch.object(verifier, "verify_tag", side_effect=ValueError("unsigned")) as check:
            with self.assertRaises(ValueError):
                verifier.main()
            check.assert_called_once()


if __name__ == "__main__":
    unittest.main()
