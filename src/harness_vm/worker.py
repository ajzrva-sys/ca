"""Host-owned launcher: drop privileges, apply Seatbelt, exec the fixed worker."""
import os
from pathlib import Path
import pwd
import sys


def run(profile):
    if os.geteuid() != 0 or not profile or len(profile) > 65536:
        raise ValueError('invalid guest worker setup')
    runner = pwd.getpwnam('ca-runner')
    if runner.pw_uid == 0:
        raise ValueError('guest worker account is privileged')
    root = Path('/Users/ca-runner/harness-worker')
    root.mkdir(mode=0o700)
    os.chown(root, runner.pw_uid, runner.pw_gid)
    for name in ('home', 'tmp', 'cargo', 'target'):
        path = root / name
        path.mkdir(mode=0o700)
        os.chown(path, runner.pw_uid, runner.pw_gid)
    env = {
        'HOME': str(root / 'home'), 'TMPDIR': str(root / 'tmp'),
        'PATH': '/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin',
        'CARGO_NET_OFFLINE': 'true', 'CARGO_HOME': str(root / 'cargo'),
        'CARGO_TARGET_DIR': str(root / 'target'), 'CARGO_BUILD_JOBS': '1',
        'RUST_TEST_THREADS': '1', 'GIT_CONFIG_NOSYSTEM': '1',
        'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_TERMINAL_PROMPT': '0',
    }
    os.chdir('/opt/ca-harness/task')
    os.setgroups([runner.pw_gid])
    os.setgid(runner.pw_gid)
    os.setuid(runner.pw_uid)
    if os.geteuid() == 0:
        raise ValueError('guest worker privilege drop failed')
    program = '/usr/bin/sandbox-exec'
    os.execve(program, [program, '-p', profile, '/opt/ca-harness/worker',
                       '--internal-harness-worker'], env)


if __name__ == '__main__':
    run(sys.argv[1])
