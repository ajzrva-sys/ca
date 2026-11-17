"""Normal task-map pressure check; bounded synthetic usage, no external calls."""
import hashlib
import json
import math
import sys
from pathlib import Path

import task_map_pty as base

READS = 7
instances = []
terminals = []


class PressureProvider(base.MapProvider):
    def __init__(self):
        self.charges = []
        super().__init__()
        instances.append(self)

    def response(self, lane, payload):
        text = base.message_text(payload)
        if ('report_structured_output' not in base.functions(payload)
                and '## Task: ' + base.TITLES[0] in text):
            assert payload.get('reasoning_effort') == 'xhigh'
            assert 'PRIVATE_CANARY' not in text
            for index in range(1, READS + 1):
                call = 'pressure-read-' + str(index)
                if base.tool_result(payload, call) is None:
                    if index == 2:
                        self.hold(self.second)
                    if index == 3:
                        self.hold(self.finish)
                    return [(call, 'read_file', {'path': 'evidence.txt',
                        'start_line': (index - 1) * 200 + 1, 'end_line': index * 200})], (
                            'FIRST-PENDING' if index == 1 else 'SECOND-PENDING' if index == 2 else '')
            self.hold(self.finish)
            return [], 'Parser inspection finished with all seven native read ranges.'
        return super().response(lane, payload)

    def model_usage(self, payload):
        # A deterministic provider charge proportional to the actual outgoing
        # request. This tests accounting/projection, not real-model tokenization.
        planner = 'report_structured_output' in base.functions(payload)
        prompt = 20 if planner else math.ceil(len(json.dumps(payload).encode()) / 4)
        self.charges.append({'planner': planner, 'prompt_tokens': prompt,
                             'completion_tokens': 10, 'total_tokens': prompt + 10})
        return prompt, 10


class PressureTerminal(base.Terminal):
    def __init__(self, *args):
        super().__init__(*args)
        terminals.append(self)


def seed_start(original):
    def seed_evidence(args, **kwargs):
        root = Path(kwargs['cwd'])
        lines = ['REPOSITORY_EVIDENCE PRIVATE_CANARY\n']
        lines += [f'row {index:04d}: saved source evidence with stable line ownership and reproducible bounded contents.\n'
                  for index in range(1, READS * 200)]
        (root / 'evidence.txt').write_text(''.join(lines))
        return original(args, **kwargs)
    return original_seed_start(seed_evidence)


original_seed_start = base.seed_start


def main():
    expected_stop = '--expect-token-stop' in sys.argv
    if expected_stop:
        sys.argv.remove('--expect-token-stop')
    output = Path(sys.argv[2]).resolve()
    base.MapProvider, base.Terminal, base.seed_start = PressureProvider, PressureTerminal, seed_start
    error = None
    try:
        base.main()
    except Exception as failure:
        error = str(failure)[:500]
    provider = instances[0]
    saved = terminals[0].saved()
    assert saved, 'missing durable task state'
    work = saved[1]['work_state']
    (output / 'pressure-work.json').write_text(json.dumps(work, indent=2))
    graph_tokens = sum(c['total_tokens'] for c in provider.charges if not c['planner'])
    usage, limits = work['agent_graph']['budget_usage'], work['agent_graph']['budget_limits']
    receipt = {'status': 'failed' if error else 'passed', 'error': error,
        'synthetic_usage': True, 'external_requests': 0, 'token_limit': limits['max_total_tokens'],
        'observed_graph_tokens': graph_tokens, 'saved_graph_tokens': usage['total_tokens'],
        'blocked': usage.get('blocked'), 'graph_status': work['agent_graph']['status'],
        'requests': len(provider.requests), 'charges': provider.charges,
        'binary_sha256': hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest()}
    (output / 'token-receipt.json').write_text(json.dumps(receipt, indent=2))
    assert limits['max_total_tokens'] == 100_000
    if expected_stop:
        assert error and usage.get('blocked') == 'total_tokens', receipt
    else:
        assert error is None, receipt
        assert work['agent_graph']['status'] == 'completed'
        assert graph_tokens == usage['total_tokens'] <= 100_000, receipt
        assert '100000 total tokens' in (output / 'review-wide.txt').read_text()
        assert any('omitted from this request to fit the shared graph token budget' in
                   base.message_text(entry['body']) for entry in provider.requests)
    print(json.dumps({key: value for key, value in receipt.items() if key != 'charges'}))


if __name__ == '__main__':
    main()
