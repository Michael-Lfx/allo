import i18n from 'i18next';

function defaultCameoName(n: number): string {
  const fallback = `参考图${n}`;
  const translated = i18n.t('videoGeneration.create.cameo.defaultName', {
    n,
    defaultValue: '参考图{{n}}',
  });
  return typeof translated === 'string' && translated.length > 0
    ? translated
    : fallback;
}

/** Avoid using camera / WeChat / AI-prompt stems as cast names (e.g. `05382109.jpg`). */
export function suggestCameoCharacterName(
  fileName: string,
  indexInBatch: number
): string {
  const stem = fileName.replace(/\.[^.]+$/, '').trim();
  const fallback = defaultCameoName(indexInBatch + 1);
  if (!stem) return fallback;
  const lower = stem.toLowerCase();
  if (/^\d{4,}$/.test(stem)) return fallback;
  if (
    /^(img[_-]?|dscn?|photo[_-]?|pic[_-]?|mmexport|wx_camera[_-]?|screenshot)/i.test(
      lower
    )
  ) {
    return fallback;
  }
  if (/^[0-9a-f]{6,}$/i.test(stem) && !/[\u4e00-\u9fff]/.test(stem)) {
    return fallback;
  }
  const tokens = stem.split(/[\s_-]+/).filter(Boolean);
  if (tokens.length >= 4) return fallback;
  const hasCjk = /[\u4e00-\u9fff]/.test(stem);
  if (!hasCjk) {
    const alnumLen = (stem.match(/[0-9a-zA-Z]/g) || []).length;
    if (alnumLen >= 28 && tokens.length >= 3) return fallback;
  } else {
    const cjkCount = (stem.match(/[\u4e00-\u9fff]/g) || []).length;
    const significant = stem.replace(/[\s_-]+/g, '').length;
    if (cjkCount >= 10 && cjkCount * 2 >= significant) {
      return fallback;
    }
  }
  return stem.slice(0, 48);
}
