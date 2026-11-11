import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import os from 'node:os';

const root = path.resolve(import.meta.dirname, '..');

function sha256File(filePath) {
  const buf = fs.readFileSync(filePath);
  return crypto.createHash('sha256').update(buf).digest('hex').toUpperCase();
}

async function main() {
  const tempDir = path.join(os.tmpdir(), `flowy-voice-t5-dev-backup-${Date.now()}`);
  fs.mkdirSync(tempDir, { recursive: true });

  const backupPlan = [
    {
      source: path.join(root, 'target', 'debug', 'Flowy.exe'),
      dest: path.join(tempDir, 'Flowy.exe'),
      name: 'Flowy.exe'
    },
    {
      source: path.join(root, 'target', 'debug', 'Flowy.pdb'),
      dest: path.join(tempDir, 'Flowy.pdb'),
      name: 'Flowy.pdb'
    },
    {
      source: path.join(root, 'target', 'debug', 'voice-companion.exe'),
      dest: path.join(tempDir, 'voice-companion.exe'),
      name: 'voice-companion.exe'
    },
    {
      source: path.join(root, 'target', 'debug', 'voice_companion.pdb'),
      dest: path.join(tempDir, 'voice_companion.pdb'),
      name: 'voice_companion.pdb'
    },
    {
      source: path.join(root, 'target', 'debug', 'ai-mate.exe'),
      dest: path.join(tempDir, 'ai-mate.exe'),
      name: 'ai-mate.exe'
    },
    {
      source: path.join(root, 'target', 'debug', 'ai_mate.pdb'),
      dest: path.join(tempDir, 'ai_mate.pdb'),
      name: 'ai_mate.pdb'
    }
  ];

  const backupRecords = [];
  for (const item of backupPlan) {
    if (fs.existsSync(item.source)) {
      const hash = sha256File(item.source);
      fs.copyFileSync(item.source, item.dest);
      backupRecords.push({
        name: item.name,
        source: item.source,
        backup: item.dest,
        sha256: hash
      });
      console.log(`[backup] ${item.name} -> ${item.dest} (${hash})`);
    } else {
      console.log(`[backup-skip] ${item.source} not found`);
    }
  }

  const resolveFrom = (...candidates) => {
    for (const c of candidates) {
      if (fs.existsSync(c)) return c;
    }
    return candidates[0];
  };

  const updates = [
    {
      from: resolveFrom(
        path.join(root, 'target', 'voice-staging', 'debug', 'Flowy.exe'),
        path.join(root, 'target', 'voice-staging', 'x86_64-pc-windows-msvc', 'debug', 'Flowy.exe')
      ),
      to: path.join(root, 'target', 'debug', 'Flowy.exe'),
      name: 'Flowy.exe'
    },
    {
      from: resolveFrom(
        path.join(root, 'target', 'voice-staging', 'debug', 'Flowy.pdb'),
        path.join(root, 'target', 'voice-staging', 'x86_64-pc-windows-msvc', 'debug', 'Flowy.pdb')
      ),
      to: path.join(root, 'target', 'debug', 'Flowy.pdb'),
      name: 'Flowy.pdb'
    },
    {
      from: resolveFrom(
        path.join(root, 'target', 'voice-staging', 'debug', 'voice-companion.exe'),
        path.join(root, 'target', 'voice-staging', 'x86_64-pc-windows-msvc', 'debug', 'voice-companion.exe'),
        path.join(root, 'target', 'voice-staging', 'debug', 'ai-mate.exe'),
        path.join(root, 'target', 'voice-staging', 'x86_64-pc-windows-msvc', 'debug', 'ai-mate.exe')
      ),
      to: path.join(root, 'target', 'debug', 'voice-companion.exe'),
      name: 'voice-companion.exe'
    },
    {
      from: resolveFrom(
        path.join(root, 'target', 'voice-staging', 'debug', 'voice_companion.pdb'),
        path.join(root, 'target', 'voice-staging', 'x86_64-pc-windows-msvc', 'debug', 'voice_companion.pdb'),
        path.join(root, 'target', 'voice-staging', 'debug', 'ai_mate.pdb'),
        path.join(root, 'target', 'voice-staging', 'x86_64-pc-windows-msvc', 'debug', 'ai_mate.pdb')
      ),
      to: path.join(root, 'target', 'debug', 'voice_companion.pdb'),
      name: 'voice_companion.pdb'
    }
  ];

  const updateRecords = [];
  for (const u of updates) {
    if (!fs.existsSync(u.from)) {
      throw new Error(`Source file missing: ${u.from}`);
    }
    fs.copyFileSync(u.from, u.to);
    const newHash = sha256File(u.to);
    updateRecords.push({
      name: u.name,
      installed: u.to,
      source: u.from,
      sha256: newHash
    });
    console.log(`[updated] ${u.to} (${newHash})`);
  }

  const restorationRecord = {
    timestamp: new Date().toISOString(),
    tempBackupDir: tempDir,
    backups: backupRecords,
    updates: updateRecords
  };

  const restorationPath = path.join(root, 'docs', 'architecture', 'voice-agent', 'execution', 'T5-default-dev-restoration.json');
  fs.writeFileSync(restorationPath, JSON.stringify(restorationRecord, null, 2), 'utf8');
  console.log(`[restoration-log] written to ${restorationPath}`);
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
