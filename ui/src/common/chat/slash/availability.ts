

/**
 * Input parameters for determining slash command list availability.
 */
export interface SlashCommandListAvailabilityInput {
  /** Current persisted conversation type. */
  conversation_type?: string;
}

/**
 * Determines whether the slash command autocomplete list should be enabled.
 *
 * Slash commands are supported by ACP and nomi agent types. The backend's
 * `/slash-commands` endpoint returns an empty list for other agent types
 * (openclaw-gateway / nanobot / remote), and also when the agent has not
 * been warmed up yet — so calling it from non-ACP/nomi types is waste.
 *
 * @param input - Conversation type and status information
 * @returns true if slash commands should be enabled
 */
export function isSlashCommandListEnabled(input: SlashCommandListAvailabilityInput): boolean {
  return input.conversation_type === 'acp' || input.conversation_type === 'nomi';
}
