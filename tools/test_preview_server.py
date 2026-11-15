"""Tests of tools/preview/preview.py: the server takes the burst of a page load and answers it.

    python -m unittest discover -s tools -p "test_*.py"
"""

import concurrent.futures
import http.client
import importlib.util
import os
import threading
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("preview", Path(__file__).parent / "preview" / "preview.py")
preview = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(preview)


class Server(unittest.TestCase):
    def test_the_listen_queue_holds_a_page_load(self):
        # main.js imports some twenty modules at once; socketserver's default of 5 refused some of them on Windows.
        self.assertGreaterEqual(preview.PreviewServer.request_queue_size, 64)

    def test_forty_parallel_requests_are_all_answered(self):
        root = Path(__file__).resolve().parent.parent
        cwd = os.getcwd()
        os.chdir(root)
        try:
            with preview.PreviewServer(("127.0.0.1", 0), preview.NoCacheHandler) as server:
                port = server.server_address[1]
                threading.Thread(target=server.serve_forever, daemon=True).start()

                def get(_):
                    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
                    try:
                        conn.request("GET", "/src/index.html")
                        response = conn.getresponse()
                        response.read()
                        return response.status, response.getheader("Cache-Control")
                    finally:
                        conn.close()

                with concurrent.futures.ThreadPoolExecutor(max_workers=40) as pool:
                    answers = list(pool.map(get, range(40)))
                server.shutdown()
        finally:
            os.chdir(cwd)
        self.assertEqual(answers, [(200, "no-store")] * 40)


if __name__ == "__main__":
    unittest.main()
