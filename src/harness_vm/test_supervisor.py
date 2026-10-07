import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch
import importlib.util
from types import SimpleNamespace

HERE = Path(__file__).parent

MOCK = '''#!/usr/bin/python3
import json, os, pathlib, sys, time
home=pathlib.Path(os.environ['TART_HOME'])/'vms'
args=sys.argv[1:]
mode=(home.parent/'mode').read_text() if (home.parent/'mode').exists() else ''
if args[0]=='list':
 if mode=='inventory_overflow': print('x'* (2*1024*1024)); sys.exit(0)
 print(json.dumps([{'Name': p.name, 'Source':'local', 'Running': (p/'running').exists(), 'State': 'running' if (p/'running').exists() else 'stopped'} for p in home.iterdir()]))
elif args[0]=='clone':
 (home/args[2]).mkdir()
 if mode=='clone_hang': time.sleep(60)
elif args[0]=='run':
 if mode=='boot_exit': sys.exit(1)
 p=home/args[-1]/'running'; p.touch()
 while p.exists(): time.sleep(.01)
elif args[0]=='stop':
 if mode=='cleanup_fail': sys.exit(1)
 (home/args[1]/'running').unlink(missing_ok=True)
elif args[0]=='delete':
 (home/args[1]).rmdir()
elif args[0]=='exec':
 print('a'*64)
else: sys.exit(1)
'''


class Lifecycle(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.tart = self.root / 'tart'
        self.tart.write_text(MOCK)
        self.tart.chmod(0o755)
        (self.root / 'vms' / 'base').mkdir(parents=True)
        self.config = {'profile': {'home': str(self.root), 'tart': str(self.tart), 'template': 'base'}, 'files': {}, 'seconds': 10}
        self.processes = []

    def tearDown(self):
        for child in self.processes:
            if child.poll() is None:
                child.stdin.close()
                child.wait(timeout=10)
            child.stdout.close()
            child.stderr.close()
        self.temp.cleanup()

    def start(self):
        child = subprocess.Popen(['/usr/bin/python3', '-I', str(HERE / 'supervisor.py')], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        self.processes.append(child)
        child.stdin.write(json.dumps(self.config).encode() + b'\n')
        child.stdin.flush()
        return child

    def event(self, child):
        self.assertTrue(select.select([child.stdout], [], [], 10)[0], 'supervisor event timed out')
        data = child.stdout.readline()
        self.assertTrue(data, 'supervisor ended without event')
        return json.loads(data)

    def assert_clean(self, child):
        result = self.event(child)
        self.assertTrue(result['stopped'], result)
        self.assertEqual(child.wait(timeout=10), 0, child.stderr.read().decode())
        self.assertEqual([p.name for p in (self.root / 'vms').iterdir()], ['base'])

    def test_owner_pipe_eof_stops_and_removes_only_owned_vm(self):
        child = self.start()
        event = self.event(child)
        self.assertTrue(event['ready'])
        self.assertTrue((self.root / 'vms' / event['name'] / 'running').exists())
        child.stdin.close()
        self.assert_clean(child)

    def test_deadline_stops_vm_without_owner_cooperation(self):
        self.config['seconds'] = 2
        child = self.start()
        self.assertTrue(self.event(child)['ready'])
        self.assert_clean(child)
        child.stdin.close()

    def test_signal_stops_vm(self):
        child = self.start()
        self.assertTrue(self.event(child)['ready'])
        child.terminate()
        self.assert_clean(child)
        child.stdin.close()

    def test_abrupt_owner_exit_still_stops_vm(self):
        # The actual owner exits without closing/waiting its supervisor. Only
        # kernel pipe EOF remains; the VM must not rely on owner destructors.
        owner = subprocess.Popen([
            '/usr/bin/python3', '-I', '-c',
            'import subprocess,sys,os; '
            'p=subprocess.Popen(["/usr/bin/python3","-I",sys.argv[1]],'
            'stdin=subprocess.PIPE,stdout=subprocess.PIPE,start_new_session=True); '
            'p.stdin.write(sys.argv[2].encode()+b"\\n"); p.stdin.flush(); '
            'print(p.stdout.readline().decode(),end="",flush=True); os._exit(7)',
            str(HERE / 'supervisor.py'), json.dumps(self.config),
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            data, error = owner.communicate(timeout=15)
            self.assertEqual(owner.returncode, 7, error.decode())
            ready = json.loads(data)
            self.assertTrue(ready['ready'])
            deadline = time.monotonic() + 10
            while (self.root / 'vms' / ready['name']).exists() and time.monotonic() < deadline:
                time.sleep(.02)
            self.assertFalse((self.root / 'vms' / ready['name']).exists())
            self.assertTrue((self.root / 'vms' / 'base').is_dir())
        finally:
            if owner.poll() is None:
                owner.kill()
                owner.wait()

    def test_template_drift_prevents_clone_and_boot(self):
        self.config['files'] = {'tart': {'device': 0}}
        child = self.start()
        result = self.event(child)
        self.assertTrue(result['stopped'])
        self.assertIsNotNone(result['error'])
        self.assertEqual(child.wait(timeout=10), 1)
        self.assertEqual([p.name for p in (self.root / 'vms').iterdir()], ['base'])
        child.stdin.close()

    def test_owner_exit_during_clone_cleans_partial_clone(self):
        (self.root / 'mode').write_text('clone_hang')
        child = self.start()
        deadline = time.monotonic() + 5
        while len(list((self.root / 'vms').iterdir())) == 1 and time.monotonic() < deadline:
            time.sleep(.02)
        self.assertEqual(len(list((self.root / 'vms').iterdir())), 2)
        child.stdin.close()
        self.assert_clean(child)

    def test_boot_failure_is_not_a_successful_lease(self):
        (self.root / 'mode').write_text('boot_exit')
        child = self.start()
        result = self.event(child)
        self.assertNotIn('ready', result)
        self.assertTrue(result['stopped'])
        self.assertIsNotNone(result['error'])
        self.assertEqual(child.wait(timeout=10), 1)
        child.stdin.close()

    def test_failed_cleanup_preserves_clone_and_reports_failure(self):
        (self.root / 'mode').write_text('cleanup_fail')
        child = self.start()
        ready = self.event(child)
        self.assertTrue(ready['ready'])
        child.stdin.close()
        result = self.event(child)
        self.assertFalse(result['stopped'])
        self.assertTrue((self.root / 'vms' / ready['name']).exists())
        self.assertEqual(child.wait(timeout=10), 1)

    def test_inventory_output_is_bounded_before_clone(self):
        (self.root / 'mode').write_text('inventory_overflow')
        child = self.start()
        result = self.event(child)
        self.assertTrue(result['stopped'])
        self.assertIn('bound', result['error'])
        self.assertEqual(child.wait(timeout=10), 1)
        self.assertEqual([p.name for p in (self.root / 'vms').iterdir()], ['base'])
        child.stdin.close()

    def test_exit_between_poll_and_kill_still_confirms_cleanup(self):
        spec = importlib.util.spec_from_file_location('supervisor', HERE / 'supervisor.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        supervisor = module.Supervisor(self.config)
        supervisor.owned = True
        (self.root / 'vms' / supervisor.name).mkdir()
        supervisor.runner = SimpleNamespace(pid=123456, poll=lambda: None, wait=lambda timeout: 0)
        try:
            with patch('os.killpg', side_effect=ProcessLookupError):
                self.assertTrue(supervisor.cleanup())
        finally:
            supervisor.scratch.cleanup()

    def test_permission_error_requires_confirmed_child_exit(self):
        spec = importlib.util.spec_from_file_location('supervisor', HERE / 'supervisor.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        supervisor = module.Supervisor(self.config)
        supervisor.owned = True
        (self.root / 'vms' / supervisor.name).mkdir()
        supervisor.runner = SimpleNamespace(pid=123456, poll=lambda: None, wait=lambda timeout: 0)
        try:
            with patch('os.killpg', side_effect=PermissionError):
                self.assertTrue(supervisor.cleanup())
            (self.root / 'vms' / supervisor.name).mkdir()
            def still_running(timeout):
                raise subprocess.TimeoutExpired('owned VM runner', timeout)
            supervisor.runner.wait = still_running
            with patch('os.killpg', side_effect=PermissionError):
                self.assertFalse(supervisor.cleanup())
            self.assertTrue((self.root / 'vms' / supervisor.name).exists())
        finally:
            supervisor.scratch.cleanup()


if __name__ == '__main__':
    unittest.main()
