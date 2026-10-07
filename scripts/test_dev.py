from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import dev  # noqa: E402


class WorkbenchLinkTests(unittest.TestCase):
    def test_keeps_the_sign_in_token_and_points_at_vite_on_localhost(self) -> None:
        self.assertEqual(
            dev.workbench_link("http://127.0.0.1:4177/?token=abc-_123"),
            f"http://localhost:{dev.WORKBENCH_PORT}/?token=abc-_123",
        )


if __name__ == "__main__":
    unittest.main()
