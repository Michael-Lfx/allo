import type { TFunction } from 'i18next';

export type ImagePromptParameters = {
  style: string | null;
  aspect: string | null;
  details: string[];
};

export const DEFAULT_IMAGE_PROMPT_PARAMETERS: ImagePromptParameters = {
  style: 'natural',
  aspect: '16:9',
  details: ['sharp'],
};

type ImageParameterOption = {
  value: string;
  labelKey: string;
};

export const IMAGE_STYLE_OPTIONS: ImageParameterOption[] = [
  { value: 'natural', labelKey: 'guid.home.imageParamStyleNatural' },
  { value: 'cinematic', labelKey: 'guid.home.imageParamStyleCinematic' },
  { value: 'product', labelKey: 'guid.home.imageParamStyleProduct' },
  { value: 'illustration', labelKey: 'guid.home.imageParamStyleIllustration' },
  { value: 'interior', labelKey: 'guid.home.imageParamStyleInterior' },
];

export const IMAGE_ASPECT_OPTIONS: ImageParameterOption[] = [
  { value: '16:9', labelKey: 'guid.home.imageParamAspectLandscape' },
  { value: '1:1', labelKey: 'guid.home.imageParamAspectSquare' },
  { value: '4:5', labelKey: 'guid.home.imageParamAspectPortrait' },
  { value: '9:16', labelKey: 'guid.home.imageParamAspectVertical' },
];

export const IMAGE_DETAIL_OPTIONS: ImageParameterOption[] = [
  { value: 'sharp', labelKey: 'guid.home.imageParamDetailSharp' },
  { value: 'bokeh', labelKey: 'guid.home.imageParamDetailBokeh' },
  { value: 'film', labelKey: 'guid.home.imageParamDetailFilm' },
  { value: 'soft', labelKey: 'guid.home.imageParamDetailSoft' },
];

const labelOf = (options: ImageParameterOption[], value: string | null, t: TFunction): string | null => {
  if (!value) return null;
  const option = options.find((entry) => entry.value === value);
  return option ? t(option.labelKey) : null;
};

export const isDefaultImagePromptParameters = (parameters: ImagePromptParameters): boolean =>
  parameters.style === DEFAULT_IMAGE_PROMPT_PARAMETERS.style &&
  parameters.aspect === DEFAULT_IMAGE_PROMPT_PARAMETERS.aspect &&
  parameters.details.length === DEFAULT_IMAGE_PROMPT_PARAMETERS.details.length &&
  parameters.details.every((value) => DEFAULT_IMAGE_PROMPT_PARAMETERS.details.includes(value));

/** Appended only at send time so the composer body stays clean and editable. */
export const buildImagePromptSuffix = (parameters: ImagePromptParameters, t: TFunction): string => {
  const parts: string[] = [];
  const style = labelOf(IMAGE_STYLE_OPTIONS, parameters.style, t);
  const aspect = labelOf(IMAGE_ASPECT_OPTIONS, parameters.aspect, t);
  const details = parameters.details
    .map((value) => labelOf(IMAGE_DETAIL_OPTIONS, value, t))
    .filter((value): value is string => Boolean(value));

  if (style) parts.push(`${t('guid.home.imageParamStyle')}: ${style}`);
  if (aspect) parts.push(`${t('guid.home.imageParamAspect')}: ${aspect}`);
  if (details.length > 0) {
    parts.push(`${t('guid.home.imageParamDetails')}: ${details.join(', ')}`);
  }

  if (parts.length === 0) return '';
  return `\n\n${t('guid.home.imageParamSummary')}: ${parts.join('; ')}.`;
};

/** Compose the actual first-message body: parameters are only a suffix for a real prompt. */
export const composeImagePromptInput = (
  input: string,
  parameters: ImagePromptParameters,
  t: TFunction,
): string => (input.trim().length > 0 ? `${input}${buildImagePromptSuffix(parameters, t)}` : input);
