"""Regression checks for archive rejection and packaged metadata."""
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

from check_cli_distribution import unpack


class ArchiveSafetyTests(unittest.TestCase):
    def test_rejects_escaping_and_wrong_package_paths(self):
        for name in ("../escape", "/absolute", "other-0.1.0/src/main.rs", "rydantic-cli-0.1.0/../escape"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root / "bad.crate"
                with tarfile.open(archive, "w:gz") as output:
                    entry = tarfile.TarInfo(name)
                    entry.size = 1
                    output.addfile(entry, io.BytesIO(b"x"))
                with self.assertRaises(ValueError):
                    unpack(archive, root / "output", "0.1.0")
                self.assertFalse((root / "output").exists())

    def test_rejects_links_and_special_files(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root / "bad.crate"
                with tarfile.open(archive, "w:gz") as output:
                    entry = tarfile.TarInfo("rydantic-cli-0.1.0/file")
                    entry.type = kind
                    entry.linkname = "/outside"
                    output.addfile(entry)
                with self.assertRaises(ValueError):
                    unpack(archive, root / "output", "0.1.0")


if __name__ == "__main__":
    unittest.main()
