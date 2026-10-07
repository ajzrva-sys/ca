"""Prepare a clean guest environment, then drop privileges before a fixed gate."""
import json
import os
from pathlib import Path
import pwd
import shutil
import sys

PROGRAMS = {
    'cargo': '/opt/homebrew/bin/cargo',
    'git': '/usr/bin/git',
    'target/release/ca': '/opt/ca-harness/project/target/release/ca',
    'target/release/codex-team': '/opt/ca-harness/project/target/release/codex-team',
}


def run(packet):
    if os.geteuid() != 0:
        raise ValueError('guest gate setup needs protected ownership')
    if set(packet) != {'program', 'arguments'} or packet['program'] not in PROGRAMS:
        raise ValueError('unknown gate program')
    arguments = packet['arguments']
    if not isinstance(arguments, list) or len(arguments) > 32 or any(not isinstance(a, str) or len(a) > 4096 or '\x00' in a for a in arguments):
        raise ValueError('invalid gate arguments')
    runner = pwd.getpwnam('ca-runner')
    if runner.pw_uid == 0:
        raise ValueError('guest gate account is privileged')
    paths = {key: Path('/Users/ca-runner') / ('harness-' + key) for key in ('home', 'tmp', 'cargo')}
    for path in paths.values():
        path.mkdir(mode=0o700)
        os.chown(path, runner.pw_uid, runner.pw_gid)
    shutil.copyfile('/opt/ca-harness/cargo/config.toml', paths['cargo'] / 'config.toml')
    (paths['cargo'] / 'config.toml').chmod(0o644)
    env = {
        'HOME': str(paths['home']), 'TMPDIR': str(paths['tmp']),
        'PATH': '/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin',
        'CARGO_HOME': str(paths['cargo']),
        'CARGO_NET_OFFLINE': 'true', 'CARGO_INCREMENTAL': '0', 'CARGO_BUILD_JOBS': '2',
        'RUST_TEST_THREADS': '4', 'GIT_CONFIG_NOSYSTEM': '1', 'GIT_ATTR_NOSYSTEM': '1',
        'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_TERMINAL_PROMPT': '0',
        'GIT_OPTIONAL_LOCKS': '0', 'GIT_NO_REPLACE_OBJECTS': '1',
        'GIT_CONFIG_COUNT': '1', 'GIT_CONFIG_KEY_0': 'safe.directory',
        'GIT_CONFIG_VALUE_0': '/opt/ca-harness/project',
        'GIT_AUTHOR_NAME': 'ca harness', 'GIT_AUTHOR_EMAIL': 'harness@localhost.invalid',
        'GIT_COMMITTER_NAME': 'ca harness', 'GIT_COMMITTER_EMAIL': 'harness@localhost.invalid',
    }
    os.chdir('/opt/ca-harness/project')
    os.setgroups([runner.pw_gid])
    os.setgid(runner.pw_gid)
    os.setuid(runner.pw_uid)
    if os.geteuid() == 0:
        raise ValueError('guest privilege drop failed')
    program = PROGRAMS[packet['program']]
    os.execve(program, [program, *arguments], env)


if __name__ == '__main__':
    run(json.loads(sys.argv[1]))
