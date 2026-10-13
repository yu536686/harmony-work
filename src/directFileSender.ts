import { encodedByteLength, FILE_CHUNK_BYTES, FILE_WINDOW_BYTES, parseFileFrame, validateAck, type FileFrame } from './fileTransferProtocol.ts';

type AttachmentInfo = { id: string; size: number; sha256: string };
type ActiveTransfer = {
  attachment: AttachmentInfo;
  sent: number;
  acked: number;
  pending: Extract<FileFrame, { type: 'data' }> | null;
  lastProgress: number;
  generation: number;
};

/** One file stream per authenticated WebRTC peer. The native layer owns paths and ticket checks. */
export class DirectFileSender {
  private mobileDeviceId: string;
  private send: (frame: FileFrame) => boolean;
  private authorize: (id: string, ticket: string, mobileDeviceId: string, offset: number) => Promise<AttachmentInfo>;
  private read: (id: string, offset: number, length: number) => Promise<string>;
  private cancelNative: (id: string) => Promise<unknown>;
  private active: ActiveTransfer | null = null;
  private generation = 0;
  private pumping = false;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private requestQueue: Promise<void> = Promise.resolve();
  private pendingRequestId: string | null = null;

  constructor(
    mobileDeviceId: string,
    send: (frame: FileFrame) => boolean,
    authorize: (id: string, ticket: string, mobileDeviceId: string, offset: number) => Promise<AttachmentInfo>,
    read: (id: string, offset: number, length: number) => Promise<string>,
    cancelNative: (id: string) => Promise<unknown>,
  ) {
    this.mobileDeviceId = mobileDeviceId;
    this.send = send;
    this.authorize = authorize;
    this.read = read;
    this.cancelNative = cancelNative;
  }

  async onFrame(raw: FileFrame): Promise<void> {
    let frame: FileFrame;
    try { frame = parseFileFrame(raw); } catch { return; }
    if (frame.type === 'request') {
      this.requestQueue = this.requestQueue.then(() => this.beginRequest(frame));
      return this.requestQueue;
    }
    if (frame.type === 'cancel') {
      if (frame.attachmentId === this.pendingRequestId && !this.active) this.generation++;
      if (frame.attachmentId === this.active?.attachment.id) await this.cancelActive();
      return;
    }
    if (!this.active || frame.attachmentId !== this.active.attachment.id || frame.type !== 'ack') return;
    try {
      this.active.acked = validateAck(this.active.acked, this.active.sent, frame.offset);
      this.active.lastProgress = Date.now();
      void this.pump();
    } catch {
      const id = this.active.attachment.id;
      await this.cancelActive();
      this.send({ type: 'error', attachmentId: id, code: 'invalid_ack' });
    }
  }

  private async beginRequest(frame: Extract<FileFrame, { type: 'request' }>): Promise<void> {
      await this.cancelActive();
      const generation = ++this.generation;
      this.pendingRequestId = frame.attachmentId;
      try {
        const attachment = await this.authorize(frame.attachmentId, frame.ticket, this.mobileDeviceId, frame.offset);
        if (generation !== this.generation) {
          await this.cancelNative(frame.attachmentId).catch(() => undefined);
          return;
        }
        if (attachment.id !== frame.attachmentId || !Number.isSafeInteger(attachment.size) ||
          attachment.size < frame.offset || attachment.size > 200 * 1024 * 1024 || !/^[0-9a-f]{64}$/i.test(attachment.sha256)) {
          throw new Error('Invalid verified attachment');
        }
        this.active = { attachment, sent: frame.offset, acked: frame.offset, pending: null,
          lastProgress: Date.now(), generation };
        void this.pump();
      } catch {
        this.send({ type: 'error', attachmentId: frame.attachmentId, code: 'not_authorized' });
      } finally {
        if (this.pendingRequestId === frame.attachmentId) this.pendingRequestId = null;
      }
  }

  stop(): void { void this.cancelActive(); }

  private schedule(): void {
    if (this.timer) return;
    this.timer = setTimeout(() => { this.timer = null; void this.pump(); }, 50);
  }

  private async cancelActive(): Promise<void> {
    this.generation++;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    const id = this.active?.attachment.id;
    this.active = null;
    if (id) await this.cancelNative(id).catch(() => undefined);
  }

  private async pump(): Promise<void> {
    if (this.pumping || !this.active) return;
    this.pumping = true;
    try {
      while (this.active) {
        const current: ActiveTransfer = this.active;
        if (Date.now() - current.lastProgress > 30_000) throw new Error('Transfer stalled');
        if (current.sent === current.attachment.size) {
          if (current.acked === current.sent) {
            if (!this.send({ type: 'complete', attachmentId: current.attachment.id,
              size: current.attachment.size, sha256: current.attachment.sha256 })) { this.schedule(); break; }
            await this.cancelActive();
          } else this.schedule();
          break;
        }
        if (current.sent - current.acked >= FILE_WINDOW_BYTES) { this.schedule(); break; }
        if (!current.pending) {
          const length = Math.min(FILE_CHUNK_BYTES, current.attachment.size - current.sent,
            FILE_WINDOW_BYTES - (current.sent - current.acked));
          const bytes = await this.read(current.attachment.id, current.sent, length);
          if (current !== this.active) break;
          if (encodedByteLength(bytes) !== length) throw new Error('Invalid local block');
          current.pending = { type: 'data', attachmentId: current.attachment.id, offset: current.sent, bytes };
        }
        if (!this.send(current.pending)) { this.schedule(); break; }
        current.sent += encodedByteLength(current.pending.bytes);
        current.pending = null;
      }
    } catch {
      const id = this.active?.attachment.id;
      await this.cancelActive();
      if (id) this.send({ type: 'error', attachmentId: id, code: 'transfer_failed' });
    } finally { this.pumping = false; }
  }
}
