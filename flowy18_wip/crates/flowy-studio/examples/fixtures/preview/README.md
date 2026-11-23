# Original preview fixtures

These project-authored geometric images and documents are provided under the
workspace MIT license. They contain no third-party artwork or embedded fonts.
The PPTX requests Microsoft YaHei by name; installed fonts are used at runtime
and are not redistributed. The PDF uses the standard Helvetica font by name.

Rebuild with Python, Pillow and python-pptx:

    python crates/flowy-studio/examples/fixtures/preview/generate.py

The PNG/JPG/WebP check image colour and aspect ratio. The three-page PDF checks
text, vector paths and pagination. The three-slide PPTX checks Chinese text,
English text, image placement and slide geometry.
