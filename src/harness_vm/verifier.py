"""Launch only the trusted baseline verifier in a fresh guest without a worker."""
import os
from pathlib import Path
import pwd


def run():
    if os.geteuid() != 0:
        raise ValueError('verifier setup needs protected ownership')
    runner = pwd.getpwnam('ca-runner')
    if runner.pw_uid == 0:
        raise ValueError('guest verifier account is privileged')
    repository = Path('/opt/ca-harness/verification')
    repository.mkdir(mode=0o755)
    repository.chmod(0o755)
    root = Path('/Users/ca-runner/harness-verifier')
    root.mkdir(mode=0o700)
    os.chown(root, runner.pw_uid, runner.pw_gid)
    for name in ('home', 'tmp'):
        path = root / name
        path.mkdir(mode=0o700)
        os.chown(path, runner.pw_uid, runner.pw_gid)
    env = {
        'HOME': str(root / 'home'), 'TMPDIR': str(root / 'tmp'),
        'PATH': '/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin',
        'CARGO_NET_OFFLINE': 'true', 'CARGO_BUILD_JOBS': '1', 'RUST_TEST_THREADS': '1',
    }
    os.chdir('/opt/ca-harness/task')
    os.setgroups([runner.pw_gid])
    os.setgid(runner.pw_gid)
    os.setuid(runner.pw_uid)
    if os.geteuid() == 0:
        raise ValueError('guest verifier privilege drop failed')
    # The baseline verifier applies the required sandbox to compilation and
    # each candidate probe. Wrapping it here would create forbidden nesting.
    program = '/opt/ca-harness/worker'
    os.execve(program, [program, '--internal-harness-verifier'], env)


if __name__ == '__main__':
    run()
