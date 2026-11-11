# Presentation runtime lock files

`requirements-core.lock.txt` is embedded into the binary (`manifest::CORE_LOCK`)
and installed with `pip install --require-hashes --only-binary=:all: --no-deps`.
One universal file covers every supported platform through environment markers.

## Regenerate

Run from this directory after editing `requirements-core.in` or bumping the
pinned ppt-master commit (compare with `skills/ppt-master/requirements.txt`):

```sh
uv pip compile requirements-core.in --universal --python-version 3.12 \
  --generate-hashes --only-binary :all: --no-header -o requirements-core.lock.txt
```

Then confirm every target still resolves to wheels only:

```sh
for p in x86_64-pc-windows-msvc x86_64-apple-darwin aarch64-apple-darwin \
         x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
  uv pip compile requirements-core.lock.txt --python-platform $p \
    --python-version 3.12 --only-binary :all: --no-header --quiet >/dev/null && echo "$p ok"
done
```

`aarch64-pc-windows-msvc` is intentionally absent: `skia-pathops` 0.9.2 ships no
`win_arm64` wheel, so Windows on ARM runs the x64 Python under emulation
(`manifest::python_build_for_host`).

The Python minor version here must match `manifest::PYTHON_VERSION`.
