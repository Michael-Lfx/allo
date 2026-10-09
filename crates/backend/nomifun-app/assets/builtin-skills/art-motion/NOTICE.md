# art-motion — Flowy builtin subset

Flowy ships this skill as **art-motion**. It vendors a **license-safe, size-safe subset** of
[alchaincyf/huashu-art-motion](https://github.com/alchaincyf/huashu-art-motion)
(MIT). The original `LICENSE` is copied beside this file.

## Shipped

- `SKILL.md` (Allo/Windows-tuned router)
- `references/` method docs and style cards
- `scripts/*.py` CLIs (`render` lives under `scripts/engine/render.py`)
- `scripts/engine/` Canvas engine, 35 style scenes, clip grammars, examples
- `schemas/` + `defaults/` for optional voice/image config
- `assets/icon.png` Flowy skill avatar

## Intentionally omitted

| Upstream path | Why |
| --- | --- |
| `assets/` showcase GIFs (~26MB) | Marketing only; not required to render |
| `scripts/engine/lib/fonts/*.woff*` (~15MB) | SIL OFL 1.1 fonts. Faithful CJK/style looks need them; the engine falls back to system fonts without them. Copy from upstream into the **project** `代码工程/lib/fonts/` if needed. `LICENSES.md` / `OFL.txt` remain here. |
| `scripts/engine/demos/` | Includes non-MIT Huashu character likeness frames |
| `scripts/engine/reference_films/` | Includes Arphic Public License stroke data |

Do **not** write new style cards back into this builtin tree (it is read-only after materialize). Copy `scripts/engine/` into the user's project first.

Volcengine TTS and paid image APIs stay opt-in and off by default. Never enable them unless the user asks.
