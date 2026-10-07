"""Read bounded task bytes through pinned descriptors; never execute candidate data."""
import json
import os
import stat
import sys

MAX_ENTRIES = 80
MAX_BYTES = 1024 * 1024
MAX_DEPTH = 8


def identity(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_mode, metadata.st_nlink,
            metadata.st_size, metadata.st_mtime_ns, metadata.st_ctime_ns)


def snapshot(root):
    files = []
    entries, remaining = 0, MAX_BYTES

    def walk(directory, prefix, depth):
        nonlocal entries, remaining
        if depth > MAX_DEPTH:
            raise ValueError('task exceeds directory depth')
        before = os.fstat(directory)
        with os.scandir(directory) as children:
            for entry in children:
                entries += 1
                if entries > MAX_ENTRIES:
                    raise ValueError('task exceeds entry bound')
                name = prefix + entry.name
                if len(name.encode('utf-8')) > 4096 or '\\' in name:
                    raise ValueError('unsupported task path')
                metadata = os.stat(entry.name, dir_fd=directory, follow_symlinks=False)
                if stat.S_ISDIR(metadata.st_mode):
                    child = os.open(entry.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                    dir_fd=directory)
                    try:
                        if identity(os.fstat(child)) != identity(metadata):
                            raise ValueError('task directory changed')
                        walk(child, name + '/', depth + 1)
                    finally:
                        os.close(child)
                elif stat.S_ISREG(metadata.st_mode):
                    if metadata.st_nlink != 1 or metadata.st_size > remaining:
                        raise ValueError('task file exceeds bound or is linked')
                    child = os.open(entry.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                                    dir_fd=directory)
                    with os.fdopen(child, 'rb') as data:
                        if identity(os.fstat(data.fileno())) != identity(metadata):
                            raise ValueError('task file changed before read')
                        content = data.read(remaining + 1)
                        if (len(content) != metadata.st_size or len(content) > remaining
                                or identity(os.fstat(data.fileno())) != identity(metadata)):
                            raise ValueError('task file changed during read')
                    remaining -= len(content)
                    files.append((name, list(content)))
                else:
                    raise ValueError('task contains a link or special file')
        if identity(os.fstat(directory)) != identity(before):
            raise ValueError('task directory changed during read')

    descriptor = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        walk(descriptor, '', 0)
    finally:
        os.close(descriptor)
    return files


if __name__ == '__main__':
    json.dump(snapshot('/opt/ca-harness/task'), sys.stdout, separators=(',', ':'))
