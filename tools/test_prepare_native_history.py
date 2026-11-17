import json
import tempfile
import unittest
from pathlib import Path
import prepare_native_history as native


class NativeImportTests(unittest.TestCase):
    def test_native_context_keeps_opaque_out_of_prose_and_preserves_tool_pairs(self):
        records = [
            {'type': 'session_meta', 'payload': {'base_instructions': {'text': 'original rules'}}},
            {'type': 'response_item', 'payload': {'type': 'reasoning', 'encrypted_content': 'OPAQUE_SENTINEL', 'summary': []}},
            {'type': 'response_item', 'payload': {'type': 'custom_tool_call', 'call_id': 'read', 'name': 'exec', 'input': 'read local evidence'}},
            {'type': 'response_item', 'payload': {'type': 'custom_tool_call_output', 'call_id': 'read', 'output': 'recorded result'}},
            {'type': 'response_item', 'payload': {'type': 'agent_message', 'text': 'retain complete unknown record', 'extra': 7}},
        ]
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / 'rollout.jsonl'
            source.write_text(''.join(json.dumps(row) + '\n' for row in records))
            before = source.read_bytes()
            envelope, visible, ledger, originals = native.convert(source, 'model')
            self.assertEqual(source.read_bytes(), before)
        self.assertEqual(envelope['input'][1]['encrypted_content'], 'OPAQUE_SENTINEL')
        self.assertNotIn('OPAQUE_SENTINEL', json.dumps(visible))
        self.assertEqual([x['call_id'] for x in envelope['input'] if 'call_id' in x], ['read', 'read'])
        self.assertIn('"extra":7', envelope['input'][-1]['content'])
        self.assertEqual(ledger[1]['opaque_receipts'][0]['bytes'], 15)
        self.assertEqual(originals[1]['encrypted_content'], 'OPAQUE_SENTINEL')

    def test_unknown_client_ciphertext_is_retained_without_forged_reasoning(self):
        rows = [{'type':'session_meta','payload':{}}, {'type':'response_item','payload':{
            'type':'agent_message','content':[{'type':'input_text','text':'relevant finding'},
                                             {'type':'encrypted_content','encrypted_content':'CLIENT_OPAQUE'}]}}]
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'rollout.jsonl';path.write_text(''.join(json.dumps(x)+'\n' for x in rows))
            envelope, visible, ledger, originals=native.convert(path,'model')
        self.assertNotIn('CLIENT_OPAQUE',json.dumps(envelope))
        self.assertNotIn('CLIENT_OPAQUE',json.dumps(visible))
        self.assertEqual(originals[0]['content'][1]['encrypted_content'],'CLIENT_OPAQUE')
        self.assertIn('relevant finding',json.dumps(envelope))
        self.assertFalse(ledger[0]['opaque_native'])

    def test_incomplete_calls_and_unrecoverable_compaction_fail(self):
        for payload in [{'type': 'custom_tool_call', 'call_id': 'missing', 'name': 'exec', 'input': ''}, None]:
            records = [{'type': 'session_meta', 'payload': {}}]
            records.append({'type': 'response_item' if payload else 'compacted', 'payload': payload or {}})
            with tempfile.TemporaryDirectory() as directory:
                source = Path(directory) / 'rollout.jsonl'
                source.write_text(''.join(json.dumps(row) + '\n' for row in records))
                with self.assertRaises(ValueError):
                    native.convert(source, 'model')


if __name__ == '__main__':
    unittest.main()
