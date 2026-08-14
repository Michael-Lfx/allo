import type { RecommendationTone } from './RecommendationCard';

export type SuggestionSignal = {
  name?: string;
  description?: string;
  content?: string;
} | null | undefined;

export const toneFromSuggestion = (suggestion: SuggestionSignal): RecommendationTone => {
  if (!suggestion) return 'none';
  const hasSignal = [suggestion.name, suggestion.description, suggestion.content].some(
    (value) => typeof value === 'string' && value.trim().length > 0
  );
  return hasSignal ? 'high' : 'none';
};
