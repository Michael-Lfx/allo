import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

import {
  ROOT,
  WORKFLOW,
  validateReleaseProductProfiles,
  validateUnibuddyInstallerBitmaps,
} from './check-release-product-profiles.mjs';

const workflow = readFileSync(`${ROOT}/${WORKFLOW}`, 'utf8');

describe('release product profile contract', () => {
  test('the checked-in workflow keeps both products isolated', () => {
    expect(validateReleaseProductProfiles(workflow)).toEqual([]);
  });

  test('missing UniBuddy tag trigger fails', () => {
    const changed = workflow.replace('      - "unibuddy-v*.*.*"\n', '');
    expect(validateReleaseProductProfiles(changed).some((item) => item.includes('UniBuddy tag trigger'))).toBe(true);
  });

  test('a ModelScope step without channel root fails', () => {
    const changed = workflow.replace('            --channel-root "$CHANNEL_ROOT" \\\n', '');
    expect(validateReleaseProductProfiles(changed).some((item) => item.includes('missing --channel-root'))).toBe(true);
  });

  test('UniBuddy installer images and main executable are not Flowy', () => {
    expect(validateUnibuddyInstallerBitmaps(ROOT)).toEqual([]);
  });

  test('a hardcoded Flowy macOS binary fails', () => {
    const changed = workflow.replace('Contents/MacOS/$PRODUCT_NAME', 'Contents/MacOS/Flowy');
    expect(validateReleaseProductProfiles(changed).some((item) => item.includes('hardcodes the Flowy binary'))).toBe(true);
  });

  test('an unconditional GitHub upload fails', () => {
    const changed = workflow.replace(
      "        if: needs.release-context.outputs.publish_github == 'true'\n",
      '',
    );
    expect(validateReleaseProductProfiles(changed).some((item) => item.includes('publish_github'))).toBe(true);
  });
});
