#!/usr/bin/python3
"""Synthetic worker used only by the explicit real-VM integration test."""
import json
import os
import struct
import sys


def read():
    size, = struct.unpack('>I', sys.stdin.buffer.read(4))
    assert 0 < size <= 4 * 1024 * 1024
    return json.loads(sys.stdin.buffer.read(size))


def write(value):
    data = json.dumps(value).encode()
    sys.stdout.buffer.write(struct.pack('>I', len(data)) + data)
    sys.stdout.buffer.flush()


setup = read()
assert setup['workspace'] == '/opt/ca-harness/task'
assert os.getcwd() == setup['workspace']
assert os.geteuid() == 550
assert os.environ['HOME'] == '/Users/ca-runner/harness-worker/home'
assert open('input.txt').read() == 'frozen task'
with open('result.txt', 'w') as output:
    output.write('guest edit')
for path in ('/opt/ca-harness/project/input.txt', '/opt/ca-harness/worker'):
    try:
        with open(path, 'a'):
            pass
        raise AssertionError('protected path was writable')
    except PermissionError:
        pass
try:
    open('/opt/ca-harness/project/input.txt').read()
    raise AssertionError('frozen source was disclosed')
except PermissionError:
    pass
for sequence in range(2):
    write({'kind': 'provider_request', 'sequence': sequence,
           'messages': [{'role': 'user', 'content': 'guest request'}], 'tools': []})
    reply = read()
    assert reply['sequence'] == sequence and reply['failed'] is False
    assert 'host reply' in json.dumps(reply['events'])
write({'kind': 'completed', 'exit_code': 0,
       'final_message': 'guest duplex verified', 'failure': None})
