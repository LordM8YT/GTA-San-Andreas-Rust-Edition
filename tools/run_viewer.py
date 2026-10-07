"""Local-only viewer server. Reads original IMG assets without extracting to disk."""
import argparse
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import sys
import webbrowser

from inspect_installation import img_index, SECTOR
from load_assets import load_pair, png

VIEWER = Path(__file__).resolve().parents[1] / 'viewer'


def original_pair(game):
    game = game.resolve(strict=True)
    source = game / 'models/gta3.img'
    if source.is_symlink() or not source.resolve().is_relative_to(game):
        raise ValueError('IMG must be inside the original installation')
    entries = img_index(source)['entries']
    assets = {}
    with source.open('rb') as stream:
        for name in ('cj_wastebin.dff', 'cj_bins.txd'):
            matching = [e for e in entries if e['name'].lower() == name]
            if len(matching) != 1:
                raise ValueError(f'Expected one archive entry: {name}')
            e = matching[0]
            if not e['classic_range_valid'] or e['archive_sectors'] or e['streaming_sectors']*SECTOR > 16*1024*1024:
                raise ValueError(f'Invalid/unsupported archive entry: {name}')
            stream.seek(e['sector_offset']*SECTOR)
            assets[name] = stream.read(e['streaming_sectors']*SECTOR)
    return assets['cj_wastebin.dff'], assets['cj_bins.txd']


def handler(model, texture):
    responses = {
        '/api/model': ('application/json', json.dumps(model).encode()),
        '/api/texture.png': ('image/png', png(texture)),
        '/': ('text/html; charset=utf-8', (VIEWER / 'index.html').read_bytes()),
        '/viewer.js': ('text/javascript', (VIEWER / 'viewer.js').read_bytes()),
    }
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            # No arbitrary path access, directory listing, remote assets, or uploads.
            entry = responses.get(self.path)
            if entry is None:
                self.send_error(404)
                return
            content_type, data = entry
            self.send_response(200)
            self.send_header('Content-Type', content_type)
            self.send_header('Content-Length', str(len(data)))
            self.send_header('Cache-Control', 'no-store')
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.send_header('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'")
            self.end_headers()
            self.wfile.write(data)
        def log_message(self, fmt, *args):
            pass
    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-dir', type=Path, default=Path(r'E:\GTA San Andreas\Grand Theft Auto San Andreas'))
    parser.add_argument('--sample-dir', type=Path, help='Development only: private extracted test pair')
    parser.add_argument('--port', type=int, default=8765)
    parser.add_argument('--no-browser', action='store_true')
    args = parser.parse_args()
    try:
        if args.sample_dir:
            dff, txd = ((args.sample_dir / name).read_bytes() for name in ('cj_wastebin.dff', 'cj_bins.txd'))
        else:
            dff, txd = original_pair(args.game_dir)
        model, texture = load_pair(dff, txd)
        server = HTTPServer(('127.0.0.1', args.port), handler(model, texture))
    except (ValueError, OSError) as exc:
        print(f'Cannot start viewer: {exc}', file=sys.stderr)
        return 1
    url = f'http://127.0.0.1:{server.server_port}/'
    print(f'{model["name"]}: {model["vertex_count"]} vertices, {model["triangle_count"]} triangles')
    print(f'Texture: {texture["name"]} ({texture["width"]}x{texture["height"]})')
    print(f'Viewer: {url}\nOriginal files read only; no disk cache. Ctrl+C to stop.', flush=True)
    if not args.no_browser:
        webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
