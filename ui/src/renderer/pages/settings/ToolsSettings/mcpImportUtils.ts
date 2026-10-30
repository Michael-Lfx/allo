import type { IMcpServer, IMcpServerTransport } from '@/common/config/storage';
import { getMcpConfigurationFields } from '@/renderer/hooks/mcp/mcpAuthConfig';
import { parseMcpJsonImport, type ParsedMcpJsonServer } from './mcpJsonImport';

export type ImportableMcpServer = Omit<IMcpServer, 'mcp_server_id' | 'created_at' | 'updated_at'> & {
  market_needs_configuration?: boolean;
  market_configuration_fields?: string[];
};

const MCP_MARKET_ORIGIN_KEY = '_nomifun_market';

export const attachMcpMarketOrigin = (originalJson: string, marketItemId: string): string => {
  let original: Record<string, unknown> = {};
  try {
    const parsed = JSON.parse(originalJson) as unknown;
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      original = parsed as Record<string, unknown>;
    }
  } catch {
    // Importable market configs normally contain valid JSON. Keep provenance
    // even if an upstream formatter produced malformed original_json.
  }
  return JSON.stringify(
    {
      ...original,
      [MCP_MARKET_ORIGIN_KEY]: { version: 1, item_id: marketItemId },
    },
    null,
    2
  );
};

export const getMcpMarketOrigin = (server: Pick<IMcpServer, 'original_json'>): string | null => {
  try {
    const parsed = JSON.parse(server.original_json) as Record<string, unknown>;
    const origin = parsed[MCP_MARKET_ORIGIN_KEY];
    if (!origin || typeof origin !== 'object' || Array.isArray(origin)) return null;
    const itemId = (origin as Record<string, unknown>).item_id;
    return typeof itemId === 'string' && itemId.trim() ? itemId : null;
  } catch {
    return null;
  }
};

/** Callers that save the stripped JSON must re-attach the origin with attachMcpMarketOrigin, or the market link is lost. */
export const stripMcpMarketOrigin = (originalJson: string): string => {
  try {
    const config = JSON.parse(originalJson) as Record<string, unknown>;
    if (!(MCP_MARKET_ORIGIN_KEY in config)) return originalJson;
    delete config[MCP_MARKET_ORIGIN_KEY];
    return JSON.stringify(config, null, 2);
  } catch {
    return originalJson;
  }
};

const SPLITTABLE_STDIO_LAUNCHERS = ['npx', 'pnpx', 'bunx', 'uvx', 'uv', 'node', 'python', 'python3', 'deno'];

const shellSplit = (input: string): string[] => {
  const tokens: string[] = [];
  let current = '';
  let quote: '"' | "'" | null = null;

  for (let index = 0; index < input.length; index += 1) {
    const char = input[index];
    if (quote) {
      if (char === quote) {
        quote = null;
        continue;
      }
      if (char === '\\' && quote === '"' && index + 1 < input.length) {
        current += input[index + 1];
        index += 1;
        continue;
      }
      current += char;
      continue;
    }

    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }

    if (char === '\\' && index + 1 < input.length) {
      current += input[index + 1];
      index += 1;
      continue;
    }

    if (/\s/.test(char)) {
      if (current) {
        tokens.push(current);
        current = '';
      }
      continue;
    }

    current += char;
  }

  if (current) {
    tokens.push(current);
  }

  return tokens;
};

const normalizeStdioCommand = (command: string, args?: string[]) => {
  const trimmed = command.trim();
  if (trimmed.length === 0 || (Array.isArray(args) && args.length > 0)) {
    return {
      command,
      args: args || [],
    };
  }

  const firstToken = trimmed.split(/\s+/)[0]?.replace(/^['"]|['"]$/g, '');
  if (!firstToken || !SPLITTABLE_STDIO_LAUNCHERS.includes(firstToken) || !/\s/.test(trimmed)) {
    return {
      command,
      args: args || [],
    };
  }

  const tokens = shellSplit(trimmed);
  if (tokens.length < 2) {
    return {
      command,
      args: args || [],
    };
  }

  return {
    command: tokens[0],
    args: tokens.slice(1),
  };
};

const buildOriginalJson = (name: string, description: string | undefined, transport: IMcpServerTransport): string => {
  const transportConfig =
    transport.type === 'stdio'
      ? {
          command: transport.command,
          args: transport.args || [],
          env: transport.env || {},
        }
      : {
          type: transport.type,
          url: transport.url,
          ...(transport.headers ? { headers: transport.headers } : {}),
        };

  return JSON.stringify(
    {
      mcpServers: {
        [name]: {
          ...(description ? { description } : {}),
          ...transportConfig,
        },
      },
    },
    null,
    2
  );
};

const normalizeParsedTransport = (transport: IMcpServerTransport): IMcpServerTransport => {
  if (transport.type !== 'stdio') {
    return transport;
  }

  const normalized = normalizeStdioCommand(transport.command, transport.args);
  return {
    ...transport,
    command: normalized.command,
    args: normalized.args,
  };
};

export const toImportableMcpServer = (
  parsedServer: ParsedMcpJsonServer,
  originalJson: string,
  enabled: boolean
): ImportableMcpServer => {
  const transport = normalizeParsedTransport(parsedServer.transport);
  const configFields = getMcpConfigurationFields(transport);
  return {
    name: parsedServer.name,
    description: parsedServer.description,
    enabled: enabled && configFields.length === 0,
    transport,
    last_test_status: 'disconnected',
    tools: [],
    original_json: originalJson || buildOriginalJson(parsedServer.name, parsedServer.description, transport),
    market_needs_configuration: configFields.length > 0,
    market_configuration_fields: configFields,
  };
};

export const toImportableMcpServersFromConfig = (config: unknown, enabled = true): ImportableMcpServer[] => {
  const parsed = parseMcpJsonImport(config);
  if (parsed.isValid === false) return [];

  return parsed.servers.map((server) =>
    toImportableMcpServer(
      server,
      JSON.stringify({ mcpServers: { [server.name]: server.originalConfig } }, null, 2),
      enabled
    )
  );
};
