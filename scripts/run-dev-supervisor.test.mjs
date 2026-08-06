import { EventEmitter } from 'node:events';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, test } from 'bun:test';

import { consumeRestartMarker } from './run-dev-restart-signal.mjs';
import { createSupervisor, waitForExit } from './run-dev-supervisor.mjs';

class FakeChild extends EventEmitter {
  constructor() {
    super();
    this.exitCode = null;
    this.pid = Math.floor(Math.random() * 10_000) + 1;
  }

  close(code = 0) {
    if (this.exitCode !== null) return;
    this.exitCode = code;
    this.emit('close', code, null);
  }
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('run-dev supervisor', () => {
  test('keeps Vite alive across a development Tauri restart', async () => {
    const vite = new FakeChild();
    const tauri = [new FakeChild(), new FakeChild()];
    tauri[0].consumeRestartSignal = () => true;
    tauri[1].consumeRestartSignal = () => false;
    let tauriStarts = 0;
    const stopped = [];
    const supervisor = createSupervisor({
      startVite: () => vite,
      startTauri: () => tauri[tauriStarts++],
      waitForVite: async () => {},
      stopProcess: (child) => {
        stopped.push(child);
        child?.close(0);
      },
    });

    const running = supervisor.run();
    await tick();
    // tao/Tauri may flatten app.exit(73) to an OS-level exit code of 0.
    // The supervisor must use the one-shot marker instead of the exit code.
    tauri[0].close(0);
    await tick();
    expect(tauriStarts).toBe(2);
    expect(vite.exitCode).toBeNull();
    expect(stopped).toEqual([]);

    tauri[1].close(0);
    expect(await running).toBe(0);
    expect(stopped).toContain(vite);
    expect(vite.exitCode).toBe(0);
  });

  test('honors a valid marker even when the Tauri child exits nonzero', async () => {
    const vite = new FakeChild();
    const tauri = [new FakeChild(), new FakeChild()];
    tauri[0].consumeRestartSignal = () => true;
    tauri[1].consumeRestartSignal = () => false;
    let tauriStarts = 0;
    const stopped = [];
    const supervisor = createSupervisor({
      startVite: () => vite,
      startTauri: () => tauri[tauriStarts++],
      waitForVite: async () => {},
      stopProcess: (child) => {
        stopped.push(child);
        child?.close(0);
      },
    });

    const running = supervisor.run();
    await tick();
    tauri[0].close(17);
    await tick();
    expect(tauriStarts).toBe(2);
    expect(vite.exitCode).toBeNull();

    tauri[1].close(0);
    expect(await running).toBe(0);
    expect(stopped).toContain(vite);
  });

  test('terminates Tauri when Vite exits unexpectedly', async () => {
    const vite = new FakeChild();
    const tauri = new FakeChild();
    const stopped = [];
    const supervisor = createSupervisor({
      startVite: () => vite,
      startTauri: () => tauri,
      waitForVite: async () => {},
      stopProcess: (child) => {
        stopped.push(child);
        child?.close(1);
      },
    });

    const running = supervisor.run();
    await tick();
    vite.close(1);
    await expect(running).rejects.toThrow('Vite exited while Tauri was running');
    expect(stopped).toContain(tauri);
    await supervisor.stop();
  });

  test('does not consume a restart marker after an explicit supervisor stop', async () => {
    const vite = new FakeChild();
    const tauri = new FakeChild();
    let markerConsumed = false;
    const supervisor = createSupervisor({
      startVite: () => vite,
      startTauri: () => {
        tauri.consumeRestartSignal = () => {
          markerConsumed = true;
          return true;
        };
        return tauri;
      },
      waitForVite: async () => {},
      stopProcess: (child) => child?.close(0),
    });

    const running = supervisor.run();
    await tick();
    await supervisor.stop();

    expect(await running).toBe(0);
    expect(markerConsumed).toBe(false);
  });

  test('cleans both processes after a normal Tauri exit', async () => {
    const vite = new FakeChild();
    const tauri = new FakeChild();
    const stopped = [];
    const supervisor = createSupervisor({
      startVite: () => vite,
      startTauri: () => tauri,
      waitForVite: async () => {},
      stopProcess: (child) => {
        stopped.push(child);
        child?.close(0);
      },
    });

    const running = supervisor.run();
    await tick();
    tauri.close(0);
    expect(await running).toBe(0);
    expect(stopped).toEqual([vite]);
  });

  test('uses a real child close event when tao exits with code 0 and leaves a marker', async () => {
    const vite = new FakeChild();
    const secondTauri = new FakeChild();
    const markerDirectory = mkdtempSync(join(tmpdir(), 'nomifun-supervisor-test-'));
    const markerPath = join(markerDirectory, 'restart.json');
    const token = 'c'.repeat(64);
    const marker = JSON.stringify({ version: 1, token, app_pid: 1234 });
    const childScript = `require('node:fs').writeFileSync(${JSON.stringify(markerPath)}, ${JSON.stringify(marker)});`;
    let tauriStarts = 0;

    try {
      const supervisor = createSupervisor({
        startVite: () => vite,
        startTauri: () => {
          if (tauriStarts++ === 0) {
            const firstTauri = spawn(process.execPath, ['-e', childScript], { stdio: 'ignore' });
            firstTauri.consumeRestartSignal = () => consumeRestartMarker({ markerPath, token });
            return firstTauri;
          }
          secondTauri.consumeRestartSignal = () => false;
          return secondTauri;
        },
        waitForVite: async () => {},
        stopProcess: (child) => {
          if (child === vite || child === secondTauri) child.close(0);
          else child.kill();
        },
      });

      const running = supervisor.run();
      for (let attempt = 0; attempt < 20 && tauriStarts < 2; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      expect(tauriStarts).toBe(2);
      expect(vite.exitCode).toBeNull();

      secondTauri.close(0);
      expect(await running).toBe(0);
    } finally {
      rmSync(markerDirectory, { recursive: true, force: true });
    }
  });
});
