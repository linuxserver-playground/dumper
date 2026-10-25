# dumper

A small static binary that extracts metadata from a container during a LinuxServer build. The shared workflows
copy it into the freshly built image, run it as the entrypoint before init, and attach what it writes to the
image's GitHub Release, where the hub and the website read it directly.

It complements syft rather than replacing it: syft stays the standard tool for packages and the SBOM attached to
the image, while dumper writes the lean, browser-ready data the website needs and no standard covers.

## Commands

| Command | Writes |
| --- | --- |
| `dumper files` | The file tree of the root filesystem as gzipped JSON (`lsio-files/1`): names, types, sizes, link targets, permissions and owners. It only calls `lstat`, so nothing is read or hashed. The format is documented at the top of [`src/files.rs`](src/files.rs). |

```sh
docker create --network none --entrypoint /.lsio-dumper/dumper <image> \
  files -o /.lsio-dumper/files.json.gz --exclude /.lsio-dumper
docker cp dumper <container>:/.lsio-dumper/dumper && docker start -a <container>
docker cp <container>:/.lsio-dumper/files.json.gz .
```

## Releases

Each release carries `dumper-x86_64` and `dumper-aarch64` (static musl builds, named after `uname -m`) and
`sha256sums.txt`. The workflows fetch the latest stable release:

```sh
curl -fsSLO "https://github.com/linuxserver-playground/dumper/releases/latest/download/dumper-$(uname -m)"
```
