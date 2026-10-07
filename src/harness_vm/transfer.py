"""Fixed guest transfer operations. Run as root only for installation/export.

No archive entry is executed, and no caller supplies an absolute destination.
Candidate programs run separately as the unprivileged ca-runner account.
"""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys
import tarfile
import tempfile

MAX_FILE = 16 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024
MAX_ARCHIVE = 640 * 1024 * 1024
MAX_ENTRIES = 50000
ROOTS = {"source": "/opt/ca-harness/project", "workspace": "/opt/ca-harness/task"}
ARTIFACTS = {"ca": "/opt/ca-harness/project/target/release/ca",
             "codex-team": "/opt/ca-harness/project/target/release/codex-team"}


def relative_path(name):
    path = PurePosixPath(name)
    if (not name or len(name.encode('utf-8')) > 4096 or path.is_absolute()
            or any(p in ('', '.', '..') for p in name.split('/'))
            or len(path.parts) > 64 or '\\' in name or '\x00' in name):
        raise ValueError('invalid transfer path')
    return path.parts


def extract(archive, destination, writable=False, owner=None):
    """Only a new private directory can be an extraction root."""
    destination = Path(destination)
    destination.mkdir(mode=0o755)
    destination.chmod(0o755)
    count, total, seen = 0, 0, set()
    with tarfile.open(fileobj=archive, mode='r:') as package:
        for member in package:
            count += 1
            parts = relative_path(member.name)
            if (count > MAX_ENTRIES or member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE)
                    or member.size < 0 or member.size > MAX_FILE
                    or member.name in seen or member.pax_headers):
                raise ValueError('unsupported or oversized transfer entry')
            if parts[0] == 'target':
                raise ValueError('build output is not a source input')
            total += member.size
            if total > MAX_TOTAL:
                raise ValueError('transfer exceeds total byte bound')
            seen.add(member.name)
            parent = destination
            for part in parts[:-1]:
                parent = parent / part
                try:
                    parent.mkdir(mode=0o755)
                    parent.chmod(0o755)
                except FileExistsError:
                    if not stat.S_ISDIR(parent.lstat().st_mode):
                        raise ValueError('transfer parent is not a directory')
            path = parent / parts[-1]
            mode = 0o755 if member.mode & 0o111 else 0o644
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
            with os.fdopen(descriptor, 'wb') as output, package.extractfile(member) as data:
                remaining = member.size
                while remaining:
                    block = data.read(min(1024 * 1024, remaining))
                    if not block:
                        raise ValueError('truncated transfer file')
                    output.write(block)
                    remaining -= len(block)
                os.fchmod(output.fileno(), mode)
    if not seen:
        raise ValueError('empty source transfer')
    # Ownership changes happen only after every archive entry has been checked.
    # The parent stays root-owned so the runner cannot replace the workspace.
    if writable and owner is not None:
        for directory, directories, files in os.walk(destination):
            os.chown(directory, *owner)
            for name in files:
                os.chown(Path(directory) / name, *owner)
    return count, total


def receive_stream(source, maximum, digest):
    result = tempfile.TemporaryFile()
    total = 0
    observed = hashlib.sha256()
    while True:
        block = source.read(min(1024 * 1024, maximum + 1 - total))
        if not block:
            break
        total += len(block)
        if total > maximum:
            result.close()
            raise ValueError('transfer stream exceeds bound')
        observed.update(block)
        result.write(block)
    if observed.hexdigest() != digest:
        result.close()
        raise ValueError('transfer stream identity mismatch')
    result.seek(0)
    return result


def install(kind, digest):
    import pwd
    if os.geteuid() != 0 or kind not in ROOTS:
        raise ValueError('invalid guest installation operation')
    runner = pwd.getpwnam('ca-runner')
    destination = Path(ROOTS[kind])
    parent = destination.parent.lstat()
    if not stat.S_ISDIR(parent.st_mode) or parent.st_uid != 0 or parent.st_mode & 0o022:
        raise ValueError('guest input parent is not protected')
    # This fixed backup belongs to the disposable clone. Do not overwrite any
    # existing transfer or copy a candidate-controlled path as administrator.
    backup = destination.with_name(destination.name + '-template')
    if backup.exists() or backup.is_symlink():
        raise ValueError('guest input was already installed')
    with receive_stream(sys.stdin.buffer, MAX_ARCHIVE, digest) as archive:
        marker = destination.parent / ('.transfer-' + kind)
        descriptor = os.open(marker, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        os.close(descriptor)
        if destination.exists() or destination.is_symlink():
            if not stat.S_ISDIR(destination.lstat().st_mode) or destination.stat().st_uid != 0:
                raise ValueError('guest input root is not protected')
            destination.rename(backup)
        count, total = extract(archive, destination, kind == 'workspace', (runner.pw_uid, runner.pw_gid))
    if kind == 'source':
        target = destination / 'target'
        target.mkdir(mode=0o755)
        target.chmod(0o755)
        os.chown(target, runner.pw_uid, runner.pw_gid)
    print(json.dumps({'sha256': digest, 'files': count, 'bytes': total}))


def open_regular(path, maximum):
    """Descriptor-relative, no-follow lookup including every directory."""
    if not Path(path).is_absolute():
        raise ValueError('artifact path must be absolute')
    parts = Path(path).parts
    directory = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in parts[1:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory)
            os.close(directory)
            directory = child
        descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
    finally:
        os.close(directory)
    observed = os.fstat(descriptor)
    if not stat.S_ISREG(observed.st_mode) or observed.st_nlink != 1 or not 0 < observed.st_size <= maximum:
        os.close(descriptor)
        raise ValueError('artifact is not a bounded regular file')
    return os.fdopen(descriptor, 'rb'), observed


def export_artifact(name):
    if name not in ARTIFACTS:
        raise ValueError('unexpected artifact name')
    data, before = open_regular(ARTIFACTS[name], MAX_TOTAL)
    with data:
        remaining = before.st_size
        while remaining:
            block = data.read(min(1024 * 1024, remaining))
            if not block:
                raise ValueError('artifact truncated during export')
            sys.stdout.buffer.write(block)
            remaining -= len(block)
        after = os.fstat(data.fileno())
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError('artifact changed during export')


def install_binary(digest, artifact=None):
    if os.geteuid() != 0:
        raise ValueError('worker installation needs protected ownership')
    if artifact is not None and artifact not in ARTIFACTS:
        raise ValueError('unexpected artifact installation')
    destination = Path(ARTIFACTS[artifact]) if artifact is not None else Path('/opt/ca-harness/worker')
    if artifact is not None:
        # Fresh source installation created target. This protected release
        # directory is for smoke/identity gates, never for subsequent builds.
        destination.parent.mkdir(mode=0o755, exist_ok=True)
        parent = destination.parent.lstat()
        if not stat.S_ISDIR(parent.st_mode) or parent.st_uid != 0 or parent.st_mode & 0o022:
            raise ValueError('artifact installation parent is not protected')
        destination.parent.chmod(0o755)
    with receive_stream(sys.stdin.buffer, MAX_TOTAL, digest) as data:
        descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o755)
        with os.fdopen(descriptor, 'wb') as output:
            while True:
                block = data.read(1024 * 1024)
                if not block:
                    break
                output.write(block)
            os.fchmod(output.fileno(), 0o755)
    print(json.dumps({'sha256': digest}))


if __name__ == '__main__':
    operation, value = sys.argv[1:3]
    if operation in ROOTS:
        install(operation, value)
    elif operation == 'worker':
        install_binary(value)
    elif operation == 'install-artifact':
        install_binary(sys.argv[3], value)
    elif operation == 'artifact':
        export_artifact(value)
    else:
        raise ValueError('unknown guest transfer operation')
