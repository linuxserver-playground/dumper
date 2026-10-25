# Changelog

## 0.1.0

First release.

- `dumper files` writes the file tree of the root filesystem it runs in as gzipped JSON, format `lsio-files/1`:
  one nested node per entry with its name, type, size, symlink or hardlink target, and permissions, uid and
  gid when they differ from the defaults.
- Only `lstat` is called: no file is read or hashed. On `linuxserver/webtop:ubuntu-kde` (248,171 entries) the
  walk takes about a second and the output is 1.8 MB.
- Hardlinks are exact, from inode numbers. Mount points other than the root are kept as empty directories, files
  Docker bind-mounts (such as `/etc/hosts`) are left out, and `--exclude` leaves out any other path.
- Static musl binaries for x86_64 and aarch64.
