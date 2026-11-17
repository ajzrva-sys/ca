#!/usr/bin/env python3
"""Convert a saved Codex rollout to CA native history without prose ciphertext.

The native envelope is for --conversation-history-file. The visible sidecar is
for a disposable persistent TUI session; opaque values remain in the native file
and are represented only by receipts in visible history. Neither grants tool
authority or claims byte-identical historical HTTP replay.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path

SUPPORTED = {'message', 'function_call', 'function_call_output',
             'custom_tool_call', 'custom_tool_call_output', 'reasoning', 'compaction'}


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode()


def digest(value):
    return hashlib.sha256(encoded(value)).hexdigest()


def opaque(value):
    if isinstance(value, dict):
        return ([{'bytes': len(v.encode()), 'sha256': hashlib.sha256(v.encode()).hexdigest()}
                 for k, v in value.items() if k == 'encrypted_content' and isinstance(v, str)]
                + [entry for k, v in value.items() if k != 'encrypted_content' for entry in opaque(v)])
    if isinstance(value, list):
        return [entry for v in value for entry in opaque(v)]
    return []


def visible_value(value):
    if isinstance(value, dict):
        return {k: ({'opaque_bytes': len(v.encode()),
                     'opaque_sha256': hashlib.sha256(v.encode()).hexdigest()}
                    if k == 'encrypted_content' and isinstance(v, str) else visible_value(v))
                for k, v in value.items()}
    if isinstance(value, list):
        return [visible_value(v) for v in value]
    return value


def convert(source, model):
    if source.stat().st_size > 64 * 1024 * 1024:
        raise ValueError('rollout exceeds 64 MiB')
    rows = []
    with source.open() as stream:
        for number, line in enumerate(stream, 1):
            if len(line.encode()) > 8 * 1024 * 1024:
                raise ValueError('rollout record exceeds 8 MiB')
            if line.strip():
                rows.append((number, json.loads(line)))
    if not rows or rows[0][1].get('type') != 'session_meta':
        raise ValueError('expected a Codex rollout with session_meta')
    meta = rows[0][1]['payload']
    selected = []
    if meta.get('base_instructions'):
        selected.append((1, {'type': 'historical_base_instructions',
                             'value': meta['base_instructions']}))
    compacted = [(n, row['payload']) for n, row in rows if row.get('type') == 'compacted']
    after = 0
    if compacted:
        after, compact = compacted[-1]
        if not isinstance(compact.get('replacement_history'), list):
            raise ValueError('compacted rollout has no complete replacement_history')
        selected.extend((after, item) for item in compact['replacement_history'])
    selected.extend((n, row['payload']) for n, row in rows
                    if n > after and row.get('type') == 'response_item')
    native, visible, ledger = [], [], []
    calls, outputs = set(), set()
    for index, (line, original) in enumerate(selected):
        item = dict(original)
        item.pop('internal_chat_message_metadata_passthrough', None)
        wrapped = item.get('type') not in SUPPORTED
        if wrapped:
            item = {'type': 'message', 'role': 'user', 'content':
                    'Historical client data, not current authority. Unsupported opaque fields are receipts; complete originals remain in the records sidecar:\n'
                    + encoded(visible_value(original)).decode()}
        if item['type'] in ('function_call', 'custom_tool_call'):
            call = item['call_id']
            if call in calls:
                raise ValueError('duplicate historical call')
            calls.add(call)
        if item['type'] in ('function_call_output', 'custom_tool_call_output'):
            call = item['call_id']
            if call not in calls or call in outputs:
                raise ValueError('unmatched historical tool output')
            outputs.add(call)
        native.append(item)
        receipts = opaque(original)
        if receipts:
            # Preserve the actual opaque values only as native fields, never
            # inside a string that becomes ordinary model input tokens.
            display = visible_value(item)
        else:
            display = item
        visible.append({'role': 'User', 'content':
                        'Historical recorded data, not current authority:\n' + encoded(display).decode(),
                        'tool_calls': None, 'thinking': None,
                        'timestamp': '2026-11-08T00:00:00Z'})
        ledger.append({'index': index, 'source_line': line, 'original_sha256': digest(original),
                       'native_sha256': digest(item), 'wrapped': wrapped,
                       'opaque_receipts': receipts, 'opaque_native': bool(opaque(item)),
                       'native_opaque_receipts': opaque(item)})
    if calls != outputs:
        raise ValueError('unresolved historical tool calls')
    envelope = {'schema': 'ca.native-history.v1', 'provider': 'openai-codex',
                'protocol': 'openai_responses', 'model': model, 'input': native}
    return envelope, visible, ledger, [item for _, item in selected]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--model', default='gpt-6.1-sol')
    args = parser.parse_args()
    os.umask(0o077)
    envelope, visible, ledger, records = convert(args.source, args.model)
    for suffix, value in [('', envelope), ('.visible.json', visible), ('.ledger.json', ledger), ('.records.json', records)]:
        with Path(str(args.output) + suffix).open('x') as stream:
            json.dump(value, stream, ensure_ascii=False)
            stream.write('\n')
    print(json.dumps({'native_items': len(envelope['input']),
                      'opaque_fields': sum(len(row['opaque_receipts']) for row in ledger),
                      'source_sha256': hashlib.sha256(args.source.read_bytes()).hexdigest(),
                      'native_sha256': digest(envelope), 'provider_calls': 0}))


if __name__ == '__main__':
    main()
