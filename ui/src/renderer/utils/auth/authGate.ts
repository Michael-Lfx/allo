

/**
 * Both desktop shell and WebUI require proving Flowy cloud account ownership
 * via Email OTP before entering the main workspace.
 */
export function requiresCloudAuthGate(): boolean {
  return true;
}

export function resolvePostLocalAuthPath(cloudAuthenticated: boolean): '/guid' | '/cloud-login' {
  if (!cloudAuthenticated) {
    return '/cloud-login';
  }
  return '/guid';
}
