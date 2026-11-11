import { expect, test } from 'bun:test';
import { readFile } from 'node:fs/promises';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import type { TMessage } from '@common/chat/chatLib';
import { parseConversationId } from '@common/types/ids';
import { buildVoiceLiveMessages } from './liveVoiceTurn';

const fixture = process.env.VOICE_WORKER_REPLAY_SNAPSHOT;
const replayTest = fixture ? test : (test as typeof test & { skip: typeof test }).skip;

replayTest('projects the actual worker replay snapshot into the bound conversation', async () => {
  const worker = JSON.parse(await readFile(fixture!, 'utf8')) as VoiceSnapshot;
  expect(worker.session_id).toBeTruthy();
  expect(worker.conversation?.[0].session_id).toBe(worker.session_id);
  expect(worker.conversation?.[0].turn_id).toBe(worker.turn_id);
  const id = parseConversationId('0190f5fe-7c00-7a00-8000-000000000002');
  // The host owns binding; this fixture only supplies that boundary's test identity.
  const snapshot = { ...worker, binding: { session_id: worker.session_id!, binding_id: 'fixture-binding', conversation_id: id, workspace: null } } as VoiceSnapshot;
  const messages = buildVoiceLiveMessages(snapshot, id, []);
  expect(messages.map(message => (message.content as {content: string}).content)).toEqual(['固定测试问题', '固定测试回复']);
  const other = parseConversationId('0190f5fe-7c00-7a00-8000-000000000003');
  expect(buildVoiceLiveMessages(snapshot, other, [])).toEqual([]);
  const withReceipt = { ...snapshot, conversation: snapshot.conversation!.map(turn => ({...turn, user_message_id: 'fixture-user', assistant_message_id: 'fixture-assistant'})) } as VoiceSnapshot;
  const persisted = [{message_id: 'fixture-user'}, {message_id: 'fixture-assistant'}] as TMessage[];
  expect(buildVoiceLiveMessages(withReceipt, id, persisted)).toEqual([]);
});
