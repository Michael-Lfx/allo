/**
 * Filename → Tauri updater platform key(s). Used when collecting from a
 * flat dist directory instead of target/<triple>/release/bundle.
 */
export function platformKeysForArtifactName(name) {
  const lower = name.toLowerCase();
  if (lower.endsWith('-setup.exe') || lower.endsWith('.exe') || lower.endsWith('.msi')) {
    if (/aarch64/i.test(name)) return ['windows-aarch64'];
    if (/_x64(?:-|_|\.)|_x86_64|_amd64/i.test(name)) return ['windows-x86_64'];
    return [];
  }
  if (lower.endsWith('.app.tar.gz')) {
    const hasArm = /aarch64|arm64/i.test(name);
    const hasX64 = /x86_64|_x64(?:-|_|\.)/i.test(name);
    if (hasArm && !hasX64) return ['darwin-aarch64'];
    if (hasX64 && !hasArm) return ['darwin-x86_64'];
    return ['darwin-x86_64', 'darwin-aarch64'];
  }
  if (lower.endsWith('.appimage') || lower.endsWith('.deb') || lower.endsWith('.rpm')) {
    if (/aarch64|arm64/i.test(name)) return ['linux-aarch64'];
    if (/_amd64|_x86_64|_x64(?:-|_|\.)/i.test(name)) return ['linux-x86_64'];
    return [];
  }
  return [];
}
