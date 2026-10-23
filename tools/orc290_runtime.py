#!/usr/bin/env python3
"""Actual-binary ORC-290 proof. Uses disposable profiles, loopback providers and workspaces."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import tempfile
import threading
import time
import uuid
from orc258_pty import FakeProvider, Walkthrough


class Client:
    def __init__(self, command, root, environment, label):
        self.events = []
        self.lines = queue.Queue()
        self.stderr = (root / (label + '-stderr.log')).open('wb')
        self.process = subprocess.Popen(command, cwd=root / 'workspace', env=environment,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr, text=True, bufsize=1)
        def read():
            for line in self.process.stdout:
                self.lines.put(json.loads(line))
            self.lines.put(None)
        self.thread = threading.Thread(target=read, daemon=True)
        self.thread.start()
        self.until('ready')

    def send(self, kind, **fields):
        self.process.stdin.write(json.dumps({'schema': 'ca.serve.v1', 'id': str(uuid.uuid4()), 'op': {'type': kind, **fields}}) + '\n')
        self.process.stdin.flush()

    def until(self, kind):
        deadline = time.monotonic() + 30
        while True:
            value = self.lines.get(timeout=max(.01, deadline - time.monotonic()))
            assert value is not None, 'service exited before ' + kind
            self.events.append(value)
            event = value['event']
            assert event['type'] != 'error', event
            if event['type'] == kind:
                return event
            if event['type'] == 'approval_requested':
                self.send('approval_response', request_id=event['request_id'], decision='allow_once')
            assert time.monotonic() < deadline, kind

    def close(self, root, label):
        if self.process.poll() is None:
            self.send('shutdown')
            self.until('goodbye')
            self.process.stdin.close()
            self.process.wait(timeout=10)
        assert self.process.returncode == 0
        (root / (label + '-events.json')).write_text(json.dumps(self.events, indent=2) + '\n')
        self.stderr.close()


def prepare(root, provider):
    (root / 'workspace').mkdir(parents=True)
    home = root / 'runtime'
    config = home / '.config/ca/config.toml'
    config.parent.mkdir(parents=True)
    config.write_text('trusted_workspaces=[' + json.dumps(str(root / 'workspace')) + ']\n'
        '[[permission_rules]]\ndecision="allow"\nkind="tool"\ntool="run_command"\n'
        '[providers.fixture]\nbase_url=' + json.dumps(provider.url) + '\napi_key=""\nmodels=["fixture-model"]\n'
        '[defaults]\nprovider="fixture"\nmodel="fixture-model"\n'
        '[formatting]\nenabled=true\npresets=["rustfmt"]\n'
        '[harness_improvement]\nenabled=false\n[learning.dream]\nenabled=false\n[learning.distill]\nenabled=false\n')
    environment = {'HOME': str(home), 'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
        'TERM': 'xterm-256color', 'LANG': 'en_US.UTF-8', 'XDG_CONFIG_HOME': str(home / '.config'),
        'XDG_DATA_HOME': str(home / '.local/share'), 'XDG_CACHE_HOME': str(home / '.cache')}
    return environment, config


def exercise(binary, root, provider, command, environment, label):
    provider.arm('ORC290_FORMAT', calls=[('orc290-format', 'write_file', {'path': 'sample.rs', 'content': 'fn main(){let n=1;println!("{n}");}\n'})])
    client = Client(command, root, environment, label)
    client.send('start_persistent_session')
    session_id = client.until('session_started')['session_id']
    client.send('submit_input', input_id='format', text='ORC290_FORMAT')
    receipt = client.until('input_admitted')['receipt']
    assert receipt['durable'] is True
    client.until('turn_completed')
    formatted = (root / 'workspace/sample.rs').read_text()
    assert '    let n = 1;' in formatted, formatted
    assert not any('Formatter warning:' in json.dumps(event) for event in client.events), client.events
    before = len(provider.requests)
    client.send('submit_input', input_id='format', text='ORC290_FORMAT')
    assert client.until('input_admitted')['receipt'] == receipt
    client.send('inspect_inbox')
    assert client.until('inbox')['inputs'][0]['status'] == 'completed'
    assert len(provider.requests) == before
    client.close(root, label)
    return session_id


def reopen(binary, root, provider, command, environment, label, session_id):
    before = len(provider.requests)
    client = Client(command, root, environment, label)
    client.send('load_persistent_session', session_id=session_id)
    client.until('session_started')
    client.send('inspect_inbox')
    assert client.until('inbox')['held'] is True
    client.send('submit_input', input_id='queued', text='ORC290_QUEUED', delivery='queue')
    client.until('input_admitted')
    client.send('inspect_inbox')
    inbox = client.until('inbox')
    assert inbox['inputs'][-1]['status'] == 'pending' and inbox['held'] is True
    assert len(provider.requests) == before
    client.send('resume_inbox')
    client.until('turn_completed')
    assert len(provider.requests) == before + 1
    client.close(root, label)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    provider = FakeProvider()
    results = {}
    try:
        root = output / 'stdio'
        environment, config = prepare(root, provider)
        config_digest = hashlib.sha256(config.read_bytes()).hexdigest()
        command = [str(binary), 'serve', '--stdio']
        session_id = exercise(binary, root, provider, command, environment, 'first')
        reopen(binary, root, provider, command, environment, 'reopened', session_id)
        assert hashlib.sha256(config.read_bytes()).hexdigest() == config_digest
        results['stdio_formatting_persistence_retry_resume'] = True
        root = output / 'websocket'
        environment, config = prepare(root, provider)
        token = root / 'fixture-token'
        token.write_text(uuid.uuid4().hex + uuid.uuid4().hex)
        token.chmod(0o600)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        server_log = (root / 'listener.log').open('wb')
        server = subprocess.Popen([str(binary), 'serve', '--listen', f'127.0.0.1:{port}', '--auth-file', str(token)],
            cwd=root / 'workspace', env=environment, stdin=subprocess.DEVNULL, stdout=server_log, stderr=server_log)
        try:
            for _ in range(100):
                try:
                    with socket.create_connection(('127.0.0.1', port), timeout=.1):
                        break
                except OSError:
                    assert server.poll() is None
                    time.sleep(.05)
            command = [str(binary), 'attach', f'ws://127.0.0.1:{port}', '--auth-file', str(token)]
            session_id = exercise(binary, root, provider, command, environment, 'first')
            reopen(binary, root, provider, command, environment, 'reopened', session_id)
            results['authenticated_websocket_formatting_persistence_retry_resume'] = True
        finally:
            server.terminate()
            server.wait(timeout=10)
            server_log.close()
        tui_root = output / 'tui'
        tui_root.mkdir()
        fixture = Walkthrough(binary, tui_root, provider)
        try:
            (fixture.workspace / 'pruning-evidence.txt').write_text(('evidence ' * 10 + '\n') * 400)
            for number in range(12):
                marker = f'ORC290_PRUNE_{number:02d}_END'
                call = f'orc290-read-{number:02d}'
                provider.arm(marker, calls=[(call, 'read_file', {'path': 'pruning-evidence.txt'})])
                fixture.command(marker, expect='ORC258_COMPLETE ' + call, timeout=20)
            fixture.command('/context', expect='Selective pruning:')
            import re
            match = re.search(r'Selective pruning: (\d+) results', fixture.screen.text())
            assert match and int(match.group(1)) > 0, fixture.screen.text()
            results['actual_tui_pruned_results'] = int(match.group(1))
            (tui_root / 'context-screen.txt').write_text(fixture.screen.text())
            results['actual_tui_context_inspection'] = True
        finally:
            fixture.close()
        results['binary_sha256'] = hashlib.sha256(binary.read_bytes()).hexdigest()
        results['provider_request_count'] = len(provider.requests)
        (output / 'receipt.json').write_text(json.dumps(results, indent=2) + '\n')
        print(json.dumps(results))
    except BaseException as error:
        (output / 'failure.json').write_text(json.dumps({'error': repr(error), 'results': results}, indent=2) + '\n')
        raise
    finally:
        (output / 'provider-requests.json').write_text(json.dumps(provider.requests, indent=2) + '\n')
        provider.close()


if __name__ == '__main__':
    main()
