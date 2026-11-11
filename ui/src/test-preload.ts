import { mock } from 'bun:test';

const noop = () => undefined;

mock.module('posthog-js', () => ({
  default: {
    init: noop,
    opt_out_capturing: noop,
    opt_in_capturing: noop,
    identify: noop,
    capture: noop,
    alias: noop,
    reset: noop,
  },
}));
