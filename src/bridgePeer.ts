/** WebRTC carries direct file bytes and optional update hints; HTTPS remains authoritative for state. */
import { parseFileFrame, type FileFrame } from './fileTransferProtocol.ts';
export type BridgeSignal = { id: number; senderDeviceId: string; kind: 'offer' | 'answer' | 'candidate'; payload: Record<string, unknown> };
export type BridgePeerOptions = {
  desktopDeviceId: string;
  mobileDeviceId: string;
  localDeviceId: string;
  remoteDeviceId: string;
  initiator: boolean;
  sendSignal: (kind: BridgeSignal['kind'], payload: Record<string, unknown>) => Promise<unknown>;
  pollSignals: (after: number) => Promise<BridgeSignal[]>;
  onHint: () => void;
  onState: (state: 'connecting' | 'direct' | 'fallback') => void;
  onFileFrame?: (frame: FileFrame) => void;
};

export class BridgePeer {
  private peer: RTCPeerConnection;
  private channel: RTCDataChannel | null = null;
  private fileChannel: RTCDataChannel | null = null;
  private timer: number | null = null;
  private cursor = 0;
  private pendingCandidates: RTCIceCandidateInit[] = [];
  private polling = false;
  private stopped = false;
  private negotiationId: string | null = null;

  constructor(private options: BridgePeerOptions) {
    this.peer = new RTCPeerConnection({ iceServers: [{ urls: 'stun:stun.l.google.com:19302' }] });
    this.peer.onicecandidate = event => {
      if (event.candidate) void this.options.sendSignal('candidate', { ...event.candidate.toJSON() }).catch(() => undefined);
    };
    this.peer.onconnectionstatechange = () => {
      options.onState(this.peer.connectionState === 'connected' ? 'direct' : this.peer.connectionState === 'failed' || this.peer.connectionState === 'disconnected' ? 'fallback' : 'connecting');
      if (this.peer.connectionState === 'failed' || this.peer.connectionState === 'disconnected') this.resumePolling();
    };
    this.peer.ondatachannel = event => this.attachChannel(event.channel);
  }

  async start() {
    this.options.onState('connecting');
    this.resumePolling();
    void this.poll();
    if (this.options.initiator) {
      await this.reconnect();
    }
  }

  async reconnect(): Promise<void> {
    if (!this.options.initiator || this.stopped || this.peer.signalingState === 'closed') return;
    this.resumePolling();
    if (this.peer.signalingState === 'have-local-offer') {
      await this.peer.setLocalDescription({ type: 'rollback' });
    }
    if (this.channel?.readyState !== 'open') {
      this.attachChannel(this.peer.createDataChannel('harmonywork-events', { ordered: true }));
    }
    if (this.fileChannel?.readyState !== 'open') {
      this.attachChannel(this.peer.createDataChannel('harmonywork-file', { ordered: true }));
    }
    this.negotiationId = crypto.randomUUID();
    const offer = await this.peer.createOffer({ iceRestart: true });
    await this.peer.setLocalDescription(offer);
    await this.options.sendSignal('offer', { type: offer.type, sdp: offer.sdp, negotiationId: this.negotiationId });
  }

  hint() {
    if (this.channel?.readyState === 'open') this.channel.send(JSON.stringify({ type: 'task-updated' }));
  }

  sendFileFrame(frame: FileFrame): boolean {
    if (this.fileChannel?.readyState !== 'open' || this.fileChannel.bufferedAmount > 1024 * 1024) return false;
    try {
      this.fileChannel.send(JSON.stringify(parseFileFrame(frame)));
      return true;
    } catch { return false; }
  }

  get fileBufferedAmount(): number { return this.fileChannel?.bufferedAmount ?? Number.POSITIVE_INFINITY; }
  get fileReady(): boolean { return this.fileChannel?.readyState === 'open'; }

  stop() {
    this.stopped = true;
    if (this.timer !== null) window.clearInterval(this.timer);
    this.channel?.close();
    this.fileChannel?.close();
    this.peer.close();
  }

  private resumePolling() {
    if (this.timer === null && !this.stopped) {
      this.timer = window.setInterval(() => void this.poll(), this.options.initiator ? 2000 : 2500);
    }
  }

  private attachChannel(channel: RTCDataChannel) {
    if (channel.label === 'harmonywork-file') {
      this.fileChannel = channel;
      channel.bufferedAmountLowThreshold = 256 * 1024;
      channel.onmessage = event => {
        try { this.options.onFileFrame?.(parseFileFrame(JSON.parse(String(event.data)))); }
        catch { /* Ignore invalid transfer frames. */ }
      };
      channel.onclose = () => {
        if (this.fileChannel === channel) this.fileChannel = null;
        this.options.onState('fallback');
        this.resumePolling();
      };
      return;
    }
    if (channel.label !== 'harmonywork-events') return;
    this.channel = channel;
    channel.onopen = () => { this.options.onState('direct'); };
    channel.onclose = () => { this.options.onState('fallback'); this.resumePolling(); };
    channel.onmessage = event => {
      try { if (JSON.parse(String(event.data))?.type === 'task-updated') this.options.onHint(); }
      catch { /* Ignore unrecognized peer messages. */ }
    };
  }

  private async poll() {
    if (this.polling || this.stopped) return;
    this.polling = true;
    try {
      const signals = await this.options.pollSignals(this.cursor);
      for (const signal of signals) {
        this.cursor = Math.max(this.cursor, signal.id);
        if (signal.senderDeviceId !== this.options.remoteDeviceId) continue;
        if (signal.kind === 'offer' && !this.options.initiator) {
          if (signal.payload.type !== 'offer' || typeof signal.payload.sdp !== 'string') continue;
          await this.peer.setRemoteDescription({ type: 'offer', sdp: signal.payload.sdp });
          await this.flushCandidates();
          const answer = await this.peer.createAnswer();
          await this.peer.setLocalDescription(answer);
          await this.options.sendSignal('answer', { type: answer.type, sdp: answer.sdp, negotiationId: signal.payload.negotiationId });
        } else if (signal.kind === 'answer' && this.options.initiator) {
          if (signal.payload.negotiationId && signal.payload.negotiationId !== this.negotiationId) continue;
          if (signal.payload.type !== 'answer' || typeof signal.payload.sdp !== 'string' || this.peer.signalingState !== 'have-local-offer') continue;
          await this.peer.setRemoteDescription({ type: 'answer', sdp: signal.payload.sdp });
          await this.flushCandidates();
        } else if (signal.kind === 'candidate' && typeof signal.payload.candidate === 'string') {
          const candidate = signal.payload as RTCIceCandidateInit;
          if (this.peer.remoteDescription) await this.peer.addIceCandidate(candidate).catch(() => undefined);
          else this.pendingCandidates.push(candidate);
        }
      }
    } catch { this.options.onState('fallback'); }
    finally { this.polling = false; }
  }

  private async flushCandidates() {
    for (const candidate of this.pendingCandidates.splice(0)) await this.peer.addIceCandidate(candidate).catch(() => undefined);
  }
}
