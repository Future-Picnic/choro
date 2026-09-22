"""Run with: python3 -B docs/test_video_library.py (no UI actions)."""
import importlib.util
import json
import re
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('library', Path(__file__).with_name('serve-video-library.py'))
library = importlib.util.module_from_spec(spec)
spec.loader.exec_module(library)


class LibraryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(tempfile.mkdtemp(prefix='choro-library-test-')).resolve()
        cls.videos = json.loads(re.search(
            r'id="completed-video-files">(.*?)</script>', library.PAGE.read_text(), re.S).group(1))
        cls.videos += json.loads(re.search(
            r'id="draft-video-files">(.*?)</script>', library.PAGE.read_text(), re.S).group(1))
        cls.videos += json.loads(re.search(
            r'id="social-video-files">(.*?)</script>', library.PAGE.read_text(), re.S).group(1))
        for video in cls.videos:
            folder = cls.root / video['folder']
            (folder / 'approved-endcards').mkdir(parents=True)
            (folder / video['file']).write_bytes(bytes(range(256)))
            poster = folder / video.get('poster', 'approved-endcards/check-v2-intro.png')
            poster.parent.mkdir(parents=True, exist_ok=True)
            poster.write_bytes(b'poster-fixture')
        cls.server = library.make_server(cls.root, 0)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.url = f'http://127.0.0.1:{cls.server.server_port}'
        page = urllib.request.urlopen(cls.url).read().decode()
        cls.token = json.loads(re.search(r'window.CHORO_VIDEO_LIBRARY=(.*?);</script>', page).group(1))['token']

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()
        # Keep this isolated tiny fixture directory; never remove user files.

    def request(self, path, headers=None, method='GET'):
        return urllib.request.urlopen(urllib.request.Request(self.url + path, headers=headers or {}, method=method))

    def status(self, path, code, headers=None, method='GET'):
        with self.assertRaises(urllib.error.HTTPError) as result:
            self.request(path, headers, method)
        self.assertEqual(result.exception.code, code)
        result.exception.close()

    def test_all_videos_and_seeking(self):
        self.assertGreater(len(self.videos), 0)
        self.assertEqual(len({video['id'] for video in self.videos}), len(self.videos))
        for video in self.videos:
            path = '/media/' + video['id']
            with self.request(path, method='HEAD') as response:
                self.assertEqual(response.headers['Content-Length'], '256')
                self.assertEqual(response.read(), b'')
            for value, expected in [('bytes=0-15', bytes(range(16))),
                                    ('bytes=250-', bytes(range(250, 256))),
                                    ('bytes=-3', bytes(range(253, 256)))]:
                with self.request(path, {'Range': value}) as response:
                    self.assertEqual(response.status, 206)
                    self.assertEqual(response.read(), expected)
            self.assertEqual(self.request('/poster/' + video['id']).read(), b'poster-fixture')
        for invalid in ['bytes=999-', 'bytes=20-1', 'bytes=-0', 'bytes=0-1,3-4', 'bytes=-']:
            self.status('/media/context', 416, {'Range': invalid})

    def test_private_scope(self):
        for path in ['/etc/passwd', '/media/../../etc/passwd', '/media/unknown', '/open-folder/context']:
            self.status(path, 404)
        self.status('/', 403, {'Host': 'attacker.example'})
        self.status('/', 403, {'Sec-Fetch-Site': 'cross-site'})
        self.status('/open-folder/context', 403, method='POST')
        self.status('/open-folder/context', 403, {'Origin': 'https://attacker.example',
                    'X-Choro-Library-Token': self.token}, 'POST')

    def test_finder_is_explicit_and_allowlisted(self):
        headers = {'Origin': self.url, 'X-Choro-Library-Token': self.token}
        with patch.object(library.subprocess, 'run') as command:
            with self.request('/open-folder/context', headers, 'POST') as response:
                self.assertEqual(response.status, 204)
            context = next(video for video in self.videos if video['id'] == 'context')
            self.assertEqual(command.call_args.args[0], ['open', str(self.root / context['folder'])])
            self.status('/open-folder/unknown', 404, headers, 'POST')
            self.assertEqual(command.call_count, 1)
        with patch.object(library.subprocess, 'run', side_effect=OSError('unavailable')):
            self.status('/open-folder/context', 503, headers, 'POST')

    def test_missing_media(self):
        with patch.object(Path, 'open', side_effect=FileNotFoundError):
            self.status('/media/context', 404)

    def test_embedded_social_plan(self):
        with self.request('/social-media-plan.html') as response:
            self.assertEqual(response.status, 200)
            self.assertIn('text/html', response.headers['Content-Type'])
            self.assertIn("frame-ancestors 'self'", response.headers['Content-Security-Policy'])
            page = response.read().decode()
            self.assertIn('Choro — Weekly social plan', page)
            self.assertIn('target="_top"', page)
        with self.request('/social-media-plan.html', method='HEAD') as response:
            self.assertEqual(response.read(), b'')
            self.assertGreater(int(response.headers['Content-Length']), 0)
        with self.request('/') as response:
            self.assertIn("frame-src 'self'", response.headers['Content-Security-Policy'])
            self.assertIn("frame-ancestors 'none'", response.headers['Content-Security-Policy'])
        with self.request('/social-launch/README.md') as response:
            self.assertIn('text/plain', response.headers['Content-Type'])
        for path in ['/social-launch/private.md', '/social-launch/../../AGENTS.md', '/social-media-plan.html/../AGENTS.md']:
            self.status(path, 404)
        self.status('/social-media-plan.html', 403, {'Sec-Fetch-Site': 'cross-site'})
        with patch.object(Path, 'read_bytes', side_effect=FileNotFoundError):
            self.status('/social-media-plan.html', 404)


if __name__ == '__main__':
    unittest.main()
