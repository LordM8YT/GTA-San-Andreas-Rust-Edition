#!/usr/bin/env python3
"""Cuts the clips from `sa-runtime --trailer-record <dir>` into one trailer,
timed to the bars of the theme from tools/trailer-music.py (96 BPM, 2.5 s bars).

    python tools/edit-trailer.py trailer/ theme.wav SARE-trailer.mp4

Needs ffmpeg on PATH. Missing clips are replaced by the next available one, so
a partial recording still produces a complete edit.
"""
import argparse
import pathlib
import subprocess
import sys

BAR = 2.5
FPS = 60
WIDTH, HEIGHT = 1920, 1080
# Letterbox to 2.39:1 for a cinematic frame.
BARS_HEIGHT = (HEIGHT - round(WIDTH / 2.39)) // 2
FONTS = [
    '/usr/share/fonts/opentype/inter/InterDisplay-Black.otf',
    'C:/Windows/Fonts/seguibl.ttf',
    'C:/Windows/Fonts/arialbd.ttf',
    '/System/Library/Fonts/Supplemental/Arial Bold.ttf',
    '/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf',
]
REPO = 'github.com/LordM8YT/SARE'

# (seconds, clip number or None for a black card, seconds into the clip, caption)
# Intro 0-10 s, groove 10-40 s, build 40-50 s, drop 50-70 s, logo 70-78 s.
EDIT = [
    (5.0, 1, 0.5, 'YOUR OWN COPY OF SAN ANDREAS'),
    (5.0, 3, 1.0, 'REBUILT FROM SCRATCH IN RUST'),
    (5.0, 2, 1.0, None),
    (5.0, 4, 1.0, None),
    (5.0, 8, 1.5, None),
    (5.0, 7, 1.5, None),
    (5.0, 6, 1.0, None),
    (5.0, 9, 0.5, None),
    (5.0, 5, 1.0, 'TEMPORAL ANTI-ALIASING  ·  WET REFLECTIONS  ·  SOFT SHADOWS'),
    (5.0, None, 0.0, 'MULTIPLAYER FOR UP TO 20 PLAYERS'),
    (2.5, 2, 5.0, None),
    (2.5, 5, 5.0, None),
    (2.5, 1, 4.0, None),
    (2.5, 7, 5.0, None),
    (2.5, 4, 4.5, None),
    (2.5, 6, 4.5, None),
    (2.5, 3, 4.5, None),
    (2.5, 10, 0.5, None),
    (8.0, 10, 3.0, 'LOGO'),
]


def font_path(explicit):
    for candidate in ([explicit] if explicit else []) + FONTS:
        if candidate and pathlib.Path(candidate).is_file():
            return candidate
    sys.exit('No title font found; pass --font <file.ttf>')


def escape(text):
    return text.replace('\\', '\\\\').replace(':', '\\:').replace("'", "\\'").replace('%', '\\%')


def caption(font, text, size, y, start, end, fade=0.4):
    alpha = (f"if(lt(t,{start}),0,if(lt(t,{start + fade}),(t-{start})/{fade},"
             f"if(lt(t,{end - fade}),1,if(lt(t,{end}),({end}-t)/{fade},0))))")
    return (f"drawtext=fontfile='{escape(font)}':text='{escape(text)}':fontsize={size}:"
            f"fontcolor=white:alpha='{alpha}':x=(w-text_w)/2:y={y}:"
            f"shadowcolor=black@0.6:shadowx=0:shadowy=3")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('clips', type=pathlib.Path)
    parser.add_argument('music', type=pathlib.Path)
    parser.add_argument('output', type=pathlib.Path)
    parser.add_argument('--font')
    args = parser.parse_args()
    font = font_path(args.font)
    clips = {int(p.name.split('-')[0]): p for p in sorted(args.clips.glob('[0-9][0-9]-*.mp4'))}
    if not clips:
        sys.exit(f'No recorded clips in {args.clips}')
    numbers = sorted(clips)

    inputs, filters, labels = [], [], []
    for index, (seconds, number, offset, text) in enumerate(EDIT):
        if number is None:
            source = f"color=c=black:s={WIDTH}x{HEIGHT}:r={FPS}:d={seconds},format=yuv420p"
            chain = f"{source}"
        else:
            if number not in clips:
                number = min(numbers, key=lambda n: (n < number, abs(n - number)))
            inputs += ['-i', str(clips[number])]
            stream = len(inputs) // 2 - 1
            chain = (f"[{stream}:v]trim=start={offset}:duration={seconds},setpts=PTS-STARTPTS,"
                     f"tpad=stop_mode=clone:stop_duration={seconds},trim=duration={seconds},"
                     f"scale={WIDTH}:{HEIGHT}:force_original_aspect_ratio=increase,"
                     f"crop={WIDTH}:{HEIGHT},fps={FPS},format=yuv420p,"
                     f"eq=contrast=1.06:saturation=1.12")
        if text == 'LOGO':
            chain += (f",fade=t=out:st=2.0:d=1.2,"
                      + caption(font, 'SARE', 190, '(h/2)-200', 2.6, seconds) + ","
                      + caption(font, 'SAN ANDREAS RUST EDITION', 52, '(h/2)+10', 3.0, seconds) + ","
                      + caption(font, REPO, 30, '(h/2)+100', 3.6, seconds) + ","
                      + caption(font, 'Requires your own copy of Grand Theft Auto: San Andreas. '
                                'Not affiliated with Rockstar Games.', 22, f'h-{BARS_HEIGHT}-50', 3.6, seconds))
        elif text:
            y = '(h/2)-40' if number is None else f'h-{BARS_HEIGHT}-110'
            chain += "," + caption(font, text, 64 if number is None else 46, y, 0.3, seconds)
        if number is not None:
            # Short fades soften the cuts at bar lines.
            chain += f",fade=t=in:st=0:d=0.12"
        label = f"v{index}"
        filters.append(f"{chain}[{label}]")
        labels.append(f"[{label}]")
    total = sum(seconds for seconds, *_ in EDIT)
    filters.append(f"{''.join(labels)}concat=n={len(labels)}:v=1:a=0,"
                   f"drawbox=x=0:y=0:w=iw:h={BARS_HEIGHT}:color=black:t=fill,"
                   f"drawbox=x=0:y=ih-{BARS_HEIGHT}:w=iw:h={BARS_HEIGHT}:color=black:t=fill,"
                   f"fade=t=in:st=0:d=1.0[video]")
    music = len(inputs) // 2
    inputs += ['-i', str(args.music)]
    filters.append(f"[{music}:a]atrim=duration={total},afade=t=out:st={total - 2}:d=2[audio]")
    command = ['ffmpeg', '-y', '-loglevel', 'error', *inputs,
               '-filter_complex', ';'.join(filters), '-map', '[video]', '-map', '[audio]',
               '-c:v', 'libx264', '-preset', 'slow', '-crf', '17', '-pix_fmt', 'yuv420p',
               '-c:a', 'aac', '-b:a', '256k', '-movflags', '+faststart', str(args.output)]
    subprocess.run(command, check=True)
    print(f'Wrote {args.output} ({total:.1f} s)')


if __name__ == '__main__':
    main()
