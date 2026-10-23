import assert from 'node:assert/strict';
import test from 'node:test';
import { DirectFileSender } from './directFileSender.ts';

const id = '123e4567-e89b-42d3-a456-426614174000';
const mobileId = '123e4567-e89b-42d3-a456-426614174001';
const ticket = 'a'.repeat(43);
const tick = () => new Promise(resolve => setTimeout(resolve, 10));

test('direct sender waits for ticket verification and sends only bounded file frames', async () => {
  const frames = [];
  const reads = [];
  const sender = new DirectFileSender(mobileId, frame => { frames.push(frame); return true; },
    async () => ({ id, size: 4, sha256: '0'.repeat(64) }),
    async (_id, offset, length) => { reads.push([offset, length]); return 'dGVzdA=='; },
    async () => undefined);
  await sender.onFrame({ type: 'request', attachmentId: id, ticket, offset: 0 });
  await tick();
  assert.deepEqual(reads, [[0, 4]]);
  assert.equal(frames[0].type, 'data');
  assert.equal(frames.some(frame => frame.type === 'complete'), false);
  await sender.onFrame({ type: 'ack', attachmentId: id, offset: 4 });
  await tick();
  assert.equal(frames.at(-1).type, 'complete');
  sender.stop();
});

test('direct sender rejects an invalid ACK and cancels native authorization', async () => {
  const frames = [];
  const cancelled = [];
  const sender = new DirectFileSender(mobileId, frame => { frames.push(frame); return true; },
    async () => ({ id, size: 4, sha256: '0'.repeat(64) }),
    async () => 'dGVzdA==',
    async current => { cancelled.push(current); });
  await sender.onFrame({ type: 'request', attachmentId: id, ticket, offset: 0 });
  await tick();
  await sender.onFrame({ type: 'ack', attachmentId: id, offset: 8 });
  assert.equal(frames.at(-1).type, 'error');
  assert.deepEqual(cancelled, [id]);
});

test('direct sender never sends more than 1 MiB without acknowledgement', async () => {
  const frames = [];
  const block = Buffer.alloc(16 * 1024, 7).toString('base64');
  const sender = new DirectFileSender(mobileId, frame => { frames.push(frame); return true; },
    async () => ({ id, size: 2 * 1024 * 1024, sha256: '0'.repeat(64) }),
    async () => block, async () => undefined);
  await sender.onFrame({ type: 'request', attachmentId: id, ticket, offset: 0 });
  for (let i = 0; i < 20 && frames.length < 64; i++) await tick();
  assert.equal(frames.filter(frame => frame.type === 'data').length, 64);
  assert.equal(frames.some(frame => frame.type === 'complete'), false);
  sender.stop();
});

test('cancelling while ticket verification is pending closes the late native grant', async () => {
  let allow;
  const verification = new Promise(resolve => { allow = resolve; });
  const frames = [];
  const cancelled = [];
  const sender = new DirectFileSender(mobileId, frame => { frames.push(frame); return true; },
    async () => verification, async () => 'dGVzdA==',
    async current => { cancelled.push(current); });
  const pending = sender.onFrame({ type: 'request', attachmentId: id, ticket, offset: 0 });
  await tick();
  await sender.onFrame({ type: 'cancel', attachmentId: id });
  allow({ id, size: 4, sha256: '0'.repeat(64) });
  await pending;
  assert.deepEqual(cancelled, [id]);
  assert.equal(frames.some(frame => frame.type === 'data'), false);
});
