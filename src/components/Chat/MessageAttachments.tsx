import { useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { File, ImageOff } from "lucide-react";
import { parsePersistedAttachments, type PersistedAttachment } from "./persistedAttachments";

function Attachment({ attachment, onOpen }: { attachment: PersistedAttachment; onOpen: (path: string) => void }) {
  const [failed, setFailed] = useState(false);
  const isImage = /^image\/(png|jpeg|webp|gif|avif|bmp)$/.test(attachment.mediaType);
  return <button type="button" className="message-attachment" onClick={() => onOpen(attachment.path)} title={attachment.path} aria-label={`预览附件 ${attachment.name}`}>
    {isImage && !failed ? <img src={convertFileSrc(attachment.path)} alt={attachment.name} loading="lazy" onError={() => setFailed(true)} /> : failed ? <ImageOff size={20} /> : <File size={20} />}
    <span>{attachment.name}</span>{failed && <small>预览不可用，点击查看文件</small>}
  </button>;
}

export default function MessageAttachments({ context, onOpen }: { context?: string | null; onOpen: (path: string) => void }) {
  const items = parsePersistedAttachments(context);
  if (!items.length) return null;
  return <div className="message-attachments" aria-label="消息附件">{items.map((attachment, index) => <Attachment key={`${attachment.path}:${index}`} attachment={attachment} onOpen={onOpen} />)}</div>;
}
