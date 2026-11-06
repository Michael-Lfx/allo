/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import aurora from './covers/aurora.webp';
import blueprint from './covers/blueprint.webp';
import chalk from './covers/chalk.webp';
import cream from './covers/cream.webp';
import data from './covers/data.webp';
import editorial from './covers/editorial.webp';
import frost from './covers/frost.webp';
import inknotes from './covers/inknotes.webp';
import inkwash from './covers/inkwash.webp';
import midnight from './covers/midnight.webp';
import navy from './covers/navy.webp';
import newsprint from './covers/newsprint.webp';
import papercut from './covers/papercut.webp';
import pictorial from './covers/pictorial.webp';
import pixel from './covers/pixel.webp';
import pop from './covers/pop.webp';
import retro from './covers/retro.webp';
import riso from './covers/riso.webp';
import sketch from './covers/sketch.webp';
import swiss from './covers/swiss.webp';
import type { SlideThemeId } from './slideThemes';

/**
 * Sample cover per theme, rendered from `scripts/slide-theme-covers/themes/<id>.html`
 * (`bun scripts/slide-theme-covers/render.mjs`). Each one uses the theme's palette
 * and ppt-master visual style with its own English sample title.
 */
const COVERS: Record<SlideThemeId, string> = {
  swiss,
  navy,
  frost,
  editorial,
  midnight,
  aurora,
  data,
  inkwash,
  sketch,
  chalk,
  blueprint,
  pictorial,
  papercut,
  retro,
  pop,
  riso,
  newsprint,
  inknotes,
  cream,
  pixel,
};

/** Decorative cover for one home theme. */
const SlideThemeThumbnail: React.FC<{ id: SlideThemeId; className?: string }> = ({ id, className }) => (
  <img className={className} src={COVERS[id]} alt='' aria-hidden='true' loading='lazy' decoding='async' draggable={false} />
);

export default SlideThemeThumbnail;
