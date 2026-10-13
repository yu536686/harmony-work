/** Bounded frames for the direct file DataChannel. File bytes never use the coordinator. */
export const FILE_CHUNK_BYTES = 16 * 1024;
export const FILE_WINDOW_BYTES = 1024 * 1024;
export const FILE_MAX_BYTES = 200 * 1024 * 1024;

export type FileFrame =
  | { type: 'request'; attachmentId: string; ticket: string; offset: number }
  | { type: 'data'; attachmentId: string; offset: number; bytes: string }
  | { type: 'ack'; attachmentId: string; offset: number }
  | { type: 'cancel'; attachmentId: string }
  | { type: 'error'; attachmentId: string; code: string }
  | { type: 'complete'; attachmentId: string; size: number; sha256: string };

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const ticket = /^[A-Za-z0-9_-]{43}$/;
const sha256 = /^[0-9a-f]{64}$/i;
const validOffset = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= FILE_MAX_BYTES;

export function encodedByteLength(bytes: string): number {
  if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(bytes)) {
    throw new Error('Invalid file block encoding');
  }
  return bytes.length / 4 * 3 - (bytes.endsWith('==') ? 2 : bytes.endsWith('=') ? 1 : 0);
}

export function parseFileFrame(value: unknown): FileFrame {
  if (!value || typeof value !== 'object') throw new Error('Invalid file frame');
  const frame = value as Record<string, unknown>;
  if (typeof frame.attachmentId !== 'string' || !uuid.test(frame.attachmentId)) throw new Error('Invalid attachment ID');
  switch (frame.type) {
    case 'request':
      if (typeof frame.ticket !== 'string' || !ticket.test(frame.ticket) || !validOffset(frame.offset)) throw new Error('Invalid transfer request');
      return { type: 'request', attachmentId: frame.attachmentId, ticket: frame.ticket, offset: frame.offset };
    case 'data':
      if (!validOffset(frame.offset) || typeof frame.bytes !== 'string' || frame.bytes.length > Math.ceil(FILE_CHUNK_BYTES / 3) * 4 ||
        encodedByteLength(frame.bytes) < 1 || encodedByteLength(frame.bytes) > FILE_CHUNK_BYTES ||
        frame.offset + encodedByteLength(frame.bytes) > FILE_MAX_BYTES) throw new Error('Invalid file block');
      return { type: 'data', attachmentId: frame.attachmentId, offset: frame.offset, bytes: frame.bytes };
    case 'ack':
      if (!validOffset(frame.offset)) throw new Error('Invalid transfer acknowledgement');
      return { type: 'ack', attachmentId: frame.attachmentId, offset: frame.offset };
    case 'complete':
      if (!validOffset(frame.size) || typeof frame.sha256 !== 'string' || !sha256.test(frame.sha256)) throw new Error('Invalid file completion');
      return { type: 'complete', attachmentId: frame.attachmentId, size: frame.size, sha256: frame.sha256.toLowerCase() };
    case 'cancel':
      return { type: 'cancel', attachmentId: frame.attachmentId };
    case 'error':
      if (typeof frame.code !== 'string' || !/^[a-z_]{1,40}$/.test(frame.code)) throw new Error('Invalid transfer error');
      return { type: 'error', attachmentId: frame.attachmentId, code: frame.code };
    default:
      throw new Error('Unknown file frame');
  }
}

export function validateNextChunk(expectedOffset: number, frame: Extract<FileFrame, { type: 'data' }>): number {
  if (frame.offset !== expectedOffset) throw new Error('File block is duplicated or out of order');
  const next = expectedOffset + encodedByteLength(frame.bytes);
  if (next > FILE_MAX_BYTES) throw new Error('File exceeds transfer limit');
  return next;
}

export function validateAck(previousAck: number, sentOffset: number, newAck: number): number {
  if (!validOffset(newAck) || newAck < previousAck || newAck > sentOffset) throw new Error('Invalid transfer acknowledgement');
  return newAck;
}
