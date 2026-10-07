"""Launch the original-asset Grove Street world viewer using Python's standard library."""
import argparse
from array import array
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sys
import webbrowser

from build_scene import build_scene
from mods import load_mods

PROJECT = Path(__file__).resolve().parents[1]
DEFAULT_GAME = Path(r'E:\GTA San Andreas\Grand Theft Auto San Andreas')


def select_game(explicit):
    if explicit is not None:
        return explicit
    settings = PROJECT/'settings.json'
    path = DEFAULT_GAME
    if settings.exists():
        if settings.stat().st_size>16384:
            raise ValueError('settings.json exceeds bounds')
        obj = json.loads(settings.read_text(encoding='utf-8'))
        if not isinstance(obj,dict) or not isinstance(obj.get('game_dir'),str):
            raise ValueError('settings.json must contain game_dir')
        path = Path(obj['game_dir'])
    if path.is_dir():
        return path
    if sys.platform != 'win32':
        raise ValueError('Use --game-dir to specify the original game installation')
    import tkinter as tk
    from tkinter import filedialog
    root = tk.Tk();root.withdraw()
    selected = filedialog.askdirectory(title='Velg din originale GTA San Andreas-mappe',mustexist=True)
    root.destroy()
    if not selected:
        raise ValueError('Ingen spillmappe ble valgt')
    return Path(selected)


def responses_for(scene,images):
    responses = {url:('image/png',data) for url,data in images.items()}
    batches = []
    for i,b in enumerate(scene['batches']):
        values = array('f',b['vertices'])
        if sys.byteorder != 'little':
            values.byteswap()
        url = f'/api/buffer/{i}.bin'
        responses[url] = ('application/octet-stream',values.tobytes())
        batches.append({k:v for k,v in b.items() if k!='vertices'}|dict(url=url,count=len(values)//12))
    document = {**scene,'batches':batches}
    responses['/api/scene'] = ('application/json',json.dumps(document,allow_nan=False).encode())
    responses['/api/texture-list'] = ('application/json',json.dumps(scene['textures']).encode())
    responses['/'] = ('text/html; charset=utf-8',(PROJECT/'viewer/world.html').read_bytes())
    responses['/world.js'] = ('text/javascript',(PROJECT/'viewer/world.js').read_bytes())
    return responses


def handler(responses):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            entry = responses.get(self.path)
            if entry is None:
                self.send_error(404);return
            kind,data = entry
            self.send_response(200)
            self.send_header('Content-Type',kind)
            self.send_header('Content-Length',str(len(data)))
            self.send_header('Cache-Control','no-store')
            self.send_header('X-Content-Type-Options','nosniff')
            self.send_header('Content-Security-Policy',"default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'")
            self.end_headers()
            try:self.wfile.write(data)
            except (BrokenPipeError,ConnectionResetError):pass
        def log_message(self,*args):pass
    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-dir',type=Path)
    parser.add_argument('--radius',type=float,default=120)
    parser.add_argument('--port',type=int,default=8766)
    parser.add_argument('--no-browser',action='store_true')
    parser.add_argument('--no-mods',action='store_true')
    args = parser.parse_args()
    server = None
    try:
        if not 50<=args.radius<=300:
            raise ValueError('PoC radius must be between 50 and 300')
        game = select_game(args.game_dir).resolve(strict=True)
        if PROJECT.resolve().is_relative_to(game):
            raise ValueError('Pakk ut runtime-prosjektet utenfor originalinstallasjonen')
        mods = load_mods(PROJECT/'mods',not args.no_mods)
        scene,images = build_scene(game,[2500,-1670],args.radius,mods)
        responses = responses_for(scene,images)
        # All assets are read-only and retained in memory. Only known routes are served.
        del images
        server = ThreadingHTTPServer(('127.0.0.1',args.port),handler(responses))
        server.daemon_threads = True
        url = f'http://127.0.0.1:{server.server_port}/'
        print(json.dumps(scene['summary'],indent=2))
        print(f'\nOpen: {url}\nOriginal files read only. Ctrl+C stops the viewer.',flush=True)
        if not args.no_browser:webbrowser.open(url)
        del scene
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    except (OSError,ValueError,KeyError,RecursionError) as exc:
        print(f'Cannot start world viewer: {exc}',file=sys.stderr)
        return 1
    finally:
        if server:server.server_close()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
