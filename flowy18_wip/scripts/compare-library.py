#!/usr/bin/env python3
"""Compare a real Library capture with its visual reference using Pillow.

Example (run from flowy18_wip):
    python scripts/compare-library.py --reference reference.png \
        --actual capture.png --actual-crop 0,32,1584,1014 \
        --output-dir .scratch/library-visual

The capture is scaled uniformly to the reference width. Only their common
top-left region is compared; coverage is reported so a short viewport cannot
silently pass as a complete match. No image is stretched independently in x/y.
The generated metrics describe pixel differences, not a visual acceptance gate.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Any

from PIL import Image, ImageChops, ImageEnhance, ImageFilter, ImageStat

Crop = tuple[int, int, int, int]


def parse_crop(value: str) -> Crop:
    """Parse a Pillow left,top,right,bottom crop rectangle."""
    try:
        parts = tuple(int(part.strip()) for part in value.split(","))
    except ValueError as error:
        raise argparse.ArgumentTypeError("Crop coordinates must be integers") from error
    if len(parts) != 4:
        raise argparse.ArgumentTypeError("Use left,top,right,bottom for a crop")
    left, top, right, bottom = parts
    if left < 0 or top < 0 or right <= left or bottom <= top:
        raise argparse.ArgumentTypeError("Crop must be a positive rectangle")
    return left, top, right, bottom


def load_crop(path: Path, crop: Crop | None) -> tuple[Image.Image, dict[str, Any]]:
    """Load RGB pixels and reject out-of-bounds crops rather than padding them."""
    with Image.open(path) as source:
        original_size = source.size
        if crop is not None and (
            crop[2] > source.width or crop[3] > source.height
        ):
            raise ValueError(f"Crop {crop} exceeds {path.name} size {source.size}")
        image = source.convert("RGB")
        if crop is not None:
            image = image.crop(crop)
    return image, {
        "path": str(path.resolve()),
        "original_size": list(original_size),
        "crop": list(crop) if crop is not None else None,
        "cropped_size": list(image.size),
    }


def dominant_background(image: Image.Image) -> tuple[int, int, int]:
    """Estimate the dominant flat surface without enumerating millions of colors."""
    sample = image.resize((128, 128), Image.Resampling.NEAREST)
    colors = sample.getcolors(maxcolors=128 * 128)
    if not colors:
        raise ValueError("Could not estimate image background")
    return max(colors, key=lambda entry: entry[0])[1]


def maximum_channel(image: Image.Image) -> Image.Image:
    red, green, blue = image.split()
    return ImageChops.lighter(ImageChops.lighter(red, green), blue)


def count_mask(mask: Image.Image) -> int:
    return mask.histogram()[255]


def measure(
    reference: Image.Image,
    actual: Image.Image,
    threshold: int,
    background_threshold: int,
) -> tuple[dict[str, Any], Image.Image, Image.Image]:
    """Measure global error and the union of both images' non-background pixels."""
    difference = ImageChops.difference(reference, actual)
    stats = ImageStat.Stat(difference)
    pixel_count = reference.width * reference.height
    significant = maximum_channel(difference).point(
        lambda value: 255 if value > threshold else 0
    )
    backgrounds = {
        "reference": dominant_background(reference),
        "actual": dominant_background(actual),
    }
    masks = []
    for name, image in (("reference", reference), ("actual", actual)):
        distance = ImageChops.difference(
            image, Image.new("RGB", image.size, backgrounds[name])
        )
        masks.append(maximum_channel(distance).point(
            lambda value: 255 if value > background_threshold else 0
        ))
    # Expand by one pixel to include anti-aliased edges beside text/icons/lines.
    content_mask = ImageChops.lighter(*masks).filter(ImageFilter.MaxFilter(3))
    content_pixels = count_mask(content_mask)
    content_stats = ImageStat.Stat(difference, mask=content_mask)
    content_significant = count_mask(ImageChops.multiply(significant, content_mask))
    mae = sum(stats.mean) / 3
    rmse = math.sqrt(sum(channel * channel for channel in stats.rms) / 3)
    content_mae = sum(content_stats.mean) / 3 if content_pixels else None
    return {
        "pixel_count": pixel_count,
        "mae_0_255": mae,
        "mae_normalized": mae / 255,
        "rmse_0_255": rmse,
        "significant_difference_threshold_0_255": threshold,
        "significant_difference_pixels": count_mask(significant),
        "significant_difference_ratio": count_mask(significant) / pixel_count,
        "estimated_background_rgb": {
            name: list(color) for name, color in backgrounds.items()
        },
        "content_background_threshold_0_255": background_threshold,
        "content_mask_pixels": content_pixels,
        "content_mask_ratio": content_pixels / pixel_count,
        "content_masked_mae_0_255": content_mae,
        "content_masked_mae_normalized": (
            content_mae / 255 if content_mae is not None else None
        ),
        "content_significant_difference_ratio": (
            content_significant / content_pixels if content_pixels else None
        ),
    }, difference, content_mask


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--actual", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--reference-crop", type=parse_crop)
    parser.add_argument("--actual-crop", type=parse_crop)
    parser.add_argument(
        "--actual-scale", type=float,
        help="Uniform scale override; by default match the cropped reference width",
    )
    parser.add_argument("--threshold", type=int, default=16)
    parser.add_argument("--background-threshold", type=int, default=12)
    arguments = parser.parse_args()
    if not 0 <= arguments.threshold <= 255:
        parser.error("--threshold must be between 0 and 255")
    if not 0 <= arguments.background_threshold <= 255:
        parser.error("--background-threshold must be between 0 and 255")
    if arguments.actual_scale is not None and (
        not math.isfinite(arguments.actual_scale) or arguments.actual_scale <= 0
    ):
        parser.error("--actual-scale must be a positive finite number")

    try:
        reference, reference_info = load_crop(
            arguments.reference, arguments.reference_crop
        )
        actual, actual_info = load_crop(arguments.actual, arguments.actual_crop)
    except (OSError, ValueError) as error:
        parser.error(str(error))

    scale = arguments.actual_scale or reference.width / actual.width
    scaled_size = (
        max(1, round(actual.width * scale)),
        max(1, round(actual.height * scale)),
    )
    scaled_actual = actual.resize(scaled_size, Image.Resampling.LANCZOS)
    common_size = (
        min(reference.width, scaled_actual.width),
        min(reference.height, scaled_actual.height),
    )
    common_box = (0, 0, *common_size)
    reference_common = reference.crop(common_box)
    actual_common = scaled_actual.crop(common_box)
    metrics, difference, content_mask = measure(
        reference_common, actual_common, arguments.threshold,
        arguments.background_threshold,
    )
    report = {
        "reference": reference_info,
        "actual": actual_info,
        "alignment": {
            "method": "uniform scale; top-left origin; common overlap only",
            "actual_scale": scale,
            "scaled_actual_size": list(scaled_size),
            "comparison_size": list(common_size),
            "reference_coverage_ratio": (
                common_size[0] * common_size[1] / (reference.width * reference.height)
            ),
            "actual_coverage_ratio": (
                common_size[0] * common_size[1] / (scaled_size[0] * scaled_size[1])
            ),
        },
        "metrics": metrics,
        "notes": [
            "MAE/RMSE use RGB channels in the 0-255 range.",
            "Significance means the largest RGB channel error exceeds the threshold.",
            "Content error uses the union of both images' non-background pixels, dilated 1 px.",
            "Large empty backgrounds can make global error low; inspect content error and overlay.",
            "Coverage below 1 means unshared viewport content was not compared.",
            "Pixel metrics do not replace inspection of font, geometry, and interaction states.",
        ],
    }
    output = arguments.output_dir
    output.mkdir(parents=True, exist_ok=True)
    reference_common.save(output / "reference.png")
    actual_common.save(output / "actual-aligned.png")
    Image.blend(reference_common, actual_common, 0.5).save(output / "overlay.png")
    ImageEnhance.Brightness(difference).enhance(4).save(output / "difference-4x.png")
    content_mask.save(output / "content-mask.png")
    (output / "metrics.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
