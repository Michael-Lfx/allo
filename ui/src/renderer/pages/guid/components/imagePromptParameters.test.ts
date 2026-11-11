import { describe, expect, test } from 'bun:test';
import type { TFunction } from 'i18next';
import {
  DEFAULT_IMAGE_PROMPT_PARAMETERS,
  buildImagePromptSuffix,
  composeImagePromptInput,
  isDefaultImagePromptParameters,
  type ImagePromptParameters,
} from './imagePromptParameters';

const t = ((key: string) => key) as unknown as TFunction;

describe('image prompt parameters', () => {
  test('adds nothing when every parameter is cleared', () => {
    expect(buildImagePromptSuffix({ style: null, aspect: null, details: [] }, t)).toBe('');
  });

  test('has visible defaults for style, aspect and details', () => {
    expect(isDefaultImagePromptParameters(DEFAULT_IMAGE_PROMPT_PARAMETERS)).toBe(true);
    expect(buildImagePromptSuffix(DEFAULT_IMAGE_PROMPT_PARAMETERS, t)).toContain(
      'guid.home.imageParamStyle: guid.home.imageParamStyleNatural',
    );
    expect(buildImagePromptSuffix(DEFAULT_IMAGE_PROMPT_PARAMETERS, t)).toContain(
      'guid.home.imageParamAspect: guid.home.imageParamAspectLandscape',
    );
  });

  test('never turns an empty input into a sendable parameter-only message', () => {
    expect(composeImagePromptInput('', DEFAULT_IMAGE_PROMPT_PARAMETERS, t)).toBe('');
    expect(composeImagePromptInput('   ', DEFAULT_IMAGE_PROMPT_PARAMETERS, t)).toBe('   ');
    expect(composeImagePromptInput('一只猫', DEFAULT_IMAGE_PROMPT_PARAMETERS, t)).toContain('一只猫');
    expect(composeImagePromptInput('一只猫', DEFAULT_IMAGE_PROMPT_PARAMETERS, t)).toContain(
      'guid.home.imageParamSummary',
    );
  });

  test('appends selected single and multi choices as one send-time suffix', () => {
    const parameters: ImagePromptParameters = {
      style: 'cinematic',
      aspect: '1:1',
      details: ['film', 'bokeh'],
    };
    const suffix = buildImagePromptSuffix(parameters, t);

    expect(suffix).toContain('guid.home.imageParamStyle: guid.home.imageParamStyleCinematic');
    expect(suffix).toContain('guid.home.imageParamAspect: guid.home.imageParamAspectSquare');
    expect(suffix).toContain(
      'guid.home.imageParamDetails: guid.home.imageParamDetailFilm, guid.home.imageParamDetailBokeh',
    );
  });
});
