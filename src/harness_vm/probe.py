"""Run only a fixed executable identity/help/version probe as ca-runner."""
import os
from pathlib import Path
import pwd
import sys


def run(flag, profile):
    if (os.geteuid() != 0 or flag not in ('--version', '--help', '--internal-harness-build-identity')
            or not profile or len(profile) > 65536):
        raise ValueError('invalid guest executable probe')
    runner = pwd.getpwnam('ca-runner')
    if runner.pw_uid == 0:
        raise ValueError('guest probe account is privileged')
    root = Path('/Users/ca-runner/harness-probe')
    root.mkdir(mode=0o700)
    os.chown(root, runner.pw_uid, runner.pw_gid)
    for name in ('home', 'tmp'):
        path = root / name
        path.mkdir(mode=0o700)
        os.chown(path, runner.pw_uid, runner.pw_gid)
    env = {
        'HOME': str(root / 'home'), 'TMPDIR': str(root / 'tmp'),
        'PATH': '/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin',
        'CARGO_NET_OFFLINE': 'true',
    }
    os.chdir(root / 'home')
    os.setgroups([runner.pw_gid])
    os.setgid(runner.pw_gid)
    os.setuid(runner.pw_uid)
    if os.geteuid() == 0:
        raise ValueError('guest probe privilege drop failed')
    program = '/usr/bin/sandbox-exec'
    os.execve(program, [program, '-p', profile, '/opt/ca-harness/worker', flag], env)


if __name__ == '__main__':
    run(*sys.argv[1:])
