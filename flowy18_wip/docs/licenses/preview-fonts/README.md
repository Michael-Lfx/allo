# Preview renderer font notices

The Rust preview dependencies are MIT / Apache-2.0: hayro 0.9 and
rpptx 0.15. The latter depends on oxml-layout 0.15.0, which embeds
metric-compatible fallback fonts. These notices are copied verbatim from that
published package's fonts/ directory. Carlito, Gelasio, Liberation, Noto and
Selawik are SIL OFL 1.1; Caladea is Apache-2.0. Include these notices with binary
distributions containing the preview engine. No fallback fonts were modified.

Installed system fonts are read locally for matching; no system font files are
checked into the repository or installed by this feature. The original geometric
test documents have their own provenance note alongside the fixtures.
