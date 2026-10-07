"""Host-side synthetic process for bounded broker transport regressions."""
import json
import struct
import sys
import time


def read():
    size, = struct.unpack('>I', sys.stdin.buffer.read(4))
    return json.loads(sys.stdin.buffer.read(size))


def write(value):
    data = json.dumps(value).encode()
    sys.stdout.buffer.write(struct.pack('>I', len(data)) + data)
    sys.stdout.buffer.flush()


setup = read()
mode = sys.argv[1]
request = {'kind': 'provider_request', 'sequence': 0,
           'messages': [{'role': 'user', 'content': 'bounded request'}], 'tools': []}
if mode == 'oversize':
    sys.stdout.buffer.write(struct.pack('>I', 4 * 1024 * 1024 + 1))
    sys.stdout.buffer.flush()
elif mode == 'truncated':
    sys.stdout.buffer.write(struct.pack('>I', 100) + b'{}')
    sys.stdout.buffer.flush()
elif mode == 'sequence':
    request['sequence'] = 1
    write(request)
elif mode == 'unknown':
    request['api_key'] = 'synthetic'
    write(request)
elif mode == 'tool':
    request['tools'] = [{'type': 'function', 'function': {
        'name': 'web_search', 'description': 'not granted', 'parameters': {}}}]
    write(request)
elif mode == 'hang':
    time.sleep(60)
else:
    write(request)
    reply = read()
    assert reply['sequence'] == 0
    assert reply['events'][-1] == 'Done'
    if mode == 'count':
        request['sequence'] = 1
        write(request)
    else:
        write({'kind': 'completed', 'exit_code': 0, 'final_message': 'done', 'failure': None})
        sys.exit(7 if mode == 'exit' else 0)
