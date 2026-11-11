# guizang-ppt-skill — Flowy builtin copy

Flowy ships this skill as **guizang-ppt-skill**. It vendors
[op7418/guizang-ppt-skill](https://github.com/op7418/guizang-ppt-skill)
by 歸藏. The original `LICENSE` (GNU Affero General Public License v3) is copied beside this file. Sponsor credits stay in `SPONSORS.md` and in the provenance note inside `SKILL.md`; do not write them into generated decks.

## Shipped

- `SKILL.md`, with Step 0 changed so the agent uses this bundled copy instead of cloning or pulling the upstream repository
- `assets/` templates, screenshot backgrounds, and the local Motion One runtime
- `references/` and `scripts/` used by the documented workflow
- `assets/icon.png` Flowy skill avatar

## Intentionally omitted

| Upstream path | Why |
| --- | --- |
| `assets/ppt-skill-showcase.png` | README marketing image, about 3.5MB, not used by the deck workflow |
| `README.md`, `README.en.md` | Install instructions that tell the agent to clone the repository |
| `.github/`, `CONTRIBUTING.md`, `docs/unification-plan.md` | Upstream project maintenance, not needed to generate a deck |

Do not write generated HTML back into this builtin tree. Write decks into the user's conversation or project directory.
