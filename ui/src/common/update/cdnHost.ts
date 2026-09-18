let updateCdnHost = 'modelscope.cn';

export function setUpdateCdnHost(host?: string | null): void {
  const trimmed = host?.trim();
  if (trimmed) updateCdnHost = trimmed;
}

export function getUpdateCdnHost(): string {
  return updateCdnHost;
}

export function resetUpdateCdnHostForTests(): void {
  updateCdnHost = 'modelscope.cn';
}
