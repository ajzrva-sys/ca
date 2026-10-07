#!/usr/bin/env python3
"""Capture actual ca PTY output as PNGs using a disposable loopback provider.

Requires Pillow and pyte. Run from the repository root:
  python3 tools/tui_screenshots.py --binary target/release/ca --output docs/images
No live provider, user configuration, or saved conversation is accessed.
"""
import argparse
import codecs
import hashlib
import json
from pathlib import Path
import tempfile

import pyte
from PIL import Image, ImageDraw, ImageFont
import orc258_pty


class CaptureScreen:
    def __init__(self, width=120, height=36):
        self.resize(width, height)
        self.decoder = codecs.getincrementaldecoder('utf-8')('replace')

    def resize(self, width, height):
        self.width, self.height = width, height
        self.vt = pyte.Screen(width, height)
        self.stream = pyte.Stream(self.vt)

    def feed(self, data):
        self.stream.feed(self.decoder.decode(data))

    def text(self):
        return '\n'.join(self.vt.display)

    def save(self, path, font_path):
        font = ImageFont.truetype(font_path, 17)
        cell_width = round(font.getlength('M'))
        cell_height, pad = 24, 20
        image = Image.new('RGB', (self.width * cell_width + pad * 2,
                                  self.height * cell_height + pad * 2), '#111318')
        draw = ImageDraw.Draw(image)
        palette = {'default': '#d9dce3', 'black': '#111318', 'red': '#e06c75',
                   'green': '#98c379', 'brown': '#e5c07b', 'blue': '#61afef',
                   'magenta': '#c678dd', 'cyan': '#56b6c2', 'white': '#d9dce3',
                   'brightblack': '#69717d', 'brightred': '#ff7b86',
                   'brightgreen': '#b4e88d', 'brightbrown': '#ffd68a',
                   'brightblue': '#84c6ff', 'brightmagenta': '#e4a0ff',
                   'brightcyan': '#80e4eb', 'brightwhite': '#ffffff'}
        def color(value, default):
            if value == 'default':
                return default
            if value in palette:
                return palette[value]
            if len(value) == 6:
                return '#' + value
            return default
        for row in range(self.height):
            for column in range(self.width):
                cell = self.vt.buffer[row][column]
                foreground = color(cell.fg, '#d9dce3')
                background = color(cell.bg, '#111318')
                if cell.reverse:
                    foreground, background = background, foreground
                x, y = pad + column * cell_width, pad + row * cell_height
                draw.rectangle((x, y, x + cell_width - 1, y + cell_height - 1), fill=background)
                if cell.data and cell.data != ' ':
                    draw.text((x, y), cell.data, font=font, fill=foreground)
        image.save(path)
        path.with_suffix('.txt').write_text(self.text() + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--font', default='/System/Library/Fonts/Menlo.ttc')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    binary = args.binary.resolve()
    # Substitute a complete VT/color parser for the text-only fixture parser.
    orc258_pty.Screen = CaptureScreen
    provider = orc258_pty.FakeProvider()
    terminal = None
    captures = []
    try:
        with tempfile.TemporaryDirectory(prefix='ca-demo-', dir='/private/tmp') as directory:
            terminal = orc258_pty.Walkthrough(binary, Path(directory).resolve(), provider,
                                             name='ca documentation demo')
            terminal.resize(120, 36)
            terminal.settle(0.5)
            if 'Trust this workspace?' in terminal.screen.text():
                terminal.write(b'y')
                terminal.settle(0.4)
            def capture(name):
                path = args.output / (name + '.png')
                terminal.screen.save(path, args.font)
                captures.append({'path': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
            capture('tui-welcome')
            prompt = 'Explain how you can help with this repository.'
            provider.arm(prompt, text='## Working with your repository\n\n'
                'I can inspect the source, explain the architecture, propose a plan, '
                'make approved edits, and run validation.\n\n'
                '- Start with a focused question or task.\n'
                '- Review the selected mode and tool permissions.\n'
                '- Check the results and validation evidence before publishing.\n\n'
                'This documentation session uses a local fixture provider.')
            terminal.command(prompt, 'local fixture provider', 15)
            terminal.settle(0.4)
            capture('tui-conversation')
            terminal.command('/usage', 'This process', 10)
            terminal.settle(0.3)
            capture('tui-usage')
            terminal.dismiss()
            terminal.command('/help', timeout=10)
            terminal.settle(0.3)
            capture('tui-help')
            terminal.dismiss()
            terminal.command('/dashboard', timeout=10)
            terminal.write(b'\x1b[B')
            terminal.settle(0.4)
            capture('tui-dashboard')
            terminal.close()
            terminal = None
    finally:
        if terminal:
            terminal.close()
        provider.close()
    (args.output / 'capture-receipt.json').write_text(json.dumps({
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'terminal_columns': 120, 'terminal_rows': 36,
        'source': 'Actual signed release binary running in a PTY; PNG rendered from its VT output.',
        'provider': 'Disposable loopback fixture; sample reply, no live model.',
        'captures': captures}, indent=2) + '\n')
    print(json.dumps({'captured': len(captures), 'output': str(args.output)}))


if __name__ == '__main__':
    main()
