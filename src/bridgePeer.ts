/** Optional direct WebRTC notification path. The HTTPS bridge remains authoritative. */
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
};

export class BridgePeer {
  private peer: RTCPeerConnection;
  private channel: RTCDataChannel | null = null;
  private timer: number | null = null;
  private cursor = 0;
  private pendingCandidates: RTCIceCandidateInit[] = [];
  private polling = false;
  private stopped = false;

  constructor(private options: BridgePeerOptions) {
    this.peer = new RTCPeerConnection({ iceServers: [{ urls: 'stun:stun.l.google.com:19302' }] });
    this.peer.onicecandidate = event => {
      if (event.candidate) void this.options.sendSignal('candidate', { ...event.candidate.toJSON() }).catch(() => undefined);
    };
    this.peer.onconnectionstatechange = () => {
      options.onState(this.peer.connectionState === 'connected' ? 'direct' : this.peer.connectionState === 'failed' || this.peer.connectionState === 'disconnected' ? 'fallback' : 'connecting');
    };
    this.peer.ondatachannel = event => this.attachChannel(event.channel);
  }

  async start() {
    this.options.onState('connecting');
    this.timer = window.setInterval(() => void this.poll(), 2000);
    void this.poll();
    if (this.options.initiator) {
      this.attachChannel(this.peer.createDataChannel('harmonywork-events', { ordered: true }));
      const offer = await this.peer.createOffer();
      await this.peer.setLocalDescription(offer);
      await this.options.sendSignal('offer', { type: offer.type, sdp: offer.sdp });
    }
  }

  hint() {
    if (this.channel?.readyState === 'open') this.channel.send(JSON.stringify({ type: 'task-updated' }));
  }

  stop() {
    this.stopped = true;
    if (this.timer !== null) window.clearInterval(this.timer);
    this.channel?.close();
    this.peer.close();
  }

  private attachChannel(channel: RTCDataChannel) {
    this.channel = channel;
    channel.onopen = () => this.options.onState('direct');
    channel.onclose = () => this.options.onState('fallback');
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
          await this.options.sendSignal('answer', { type: answer.type, sdp: answer.sdp });
        } else if (signal.kind === 'answer' && this.options.initiator) {
          if (signal.payload.type !== 'answer' || typeof signal.payload.sdp !== 'string' || this.peer.signalingState !== 'have-local-offer') continue;
          await this.peer.setRemoteDescription({ type: 'answer', sdp: signal.payload.sdp });
          await this.flushCandidates();
        } else if (signal.kind === 'candidate' && typeof signal.payload.candidate === 'string') {
          const candidate = signal.payload as RTCIceCandidateInit;
          if (this.peer.remoteDescription) await this.peer.addIceCandidate(candidate);
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
