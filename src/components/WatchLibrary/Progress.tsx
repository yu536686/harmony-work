import { CheckCircle2, CircleAlert, LoaderCircle, Pause } from 'lucide-react';
import type { WatchTask, WatchUploadTask } from '../../services/tauri/watchLibrary';

export function TaskProgress({ task }: { task: WatchTask }) {
  const running = ['queued', 'running'].includes(task.status) || task.running === true;
  const failed = task.status === 'failed';
  const total = Number.isFinite(task.total) && task.total! > 0 ? task.total! : undefined;
  const done = total ? Math.max(0, Math.min(total, task.done || 0)) : undefined;
  const labels: Record<string, string> = { queued: '任务已排队', running: '正在处理', succeeded: '处理结束', failed: '处理失败' };
  return <div className="watch-progress-panel"><div className="watch-progress-head">{running ? <LoaderCircle className="watch-spinner" size={17} /> : failed ? <CircleAlert size={17} /> : <CheckCircle2 size={17} />}<strong>{labels[task.status] || task.status}</strong>{total != null && <span>{done} / {total}</span>}</div>
    <p role="status">{task.stage || (running ? '正在等待服务端返回处理进度' : failed ? '请查看错误信息后重试' : '结果已返回')}</p>
    {running && <progress aria-label="任务处理进度" max={total || 1} value={done} />}
    <ol className="watch-stage-list" aria-label="任务阶段">{['已提交', '服务端处理', '结果返回'].map((label, i) => <li key={label} data-state={failed ? (i === 1 ? 'error' : i === 0 ? 'done' : 'waiting') : task.status === 'succeeded' ? 'done' : i < (task.status === 'queued' ? 0 : 1) ? 'done' : i === (task.status === 'queued' ? 0 : 1) ? 'active' : 'waiting'}>{label}</li>)}</ol>
  </div>;
}

export function UploadProgress({ task }: { task: WatchUploadTask }) {
  const transferred = Math.max(0, Math.min(task.size, task.uploaded));
  const percent = task.size > 0 ? Math.floor(transferred / task.size * 100) : 0;
  const busy = ['queued', 'verifying', 'uploading', 'saving', 'cancelling'].includes(task.status);
  const labels: Record<string, string> = { queued: '等待上传', verifying: '正在校验源文件', uploading: `传输 ${percent}%`, saving: '文件已传输，正在保存与入库', completed: '已完成保存与入库', paused: '已暂停，可继续上传', failed: '上传未完成，请重试', cancelled: '已取消', cancelling: '正在取消' };
  return <div className="watch-progress-panel"><div className="watch-progress-head">{task.status === 'completed' ? <CheckCircle2 size={17} /> : task.status === 'failed' ? <CircleAlert size={17} /> : task.status === 'paused' ? <Pause size={17} /> : busy ? <LoaderCircle className="watch-spinner" size={17} /> : null}<span role="status">{labels[task.status] || task.status}</span></div>
    <progress aria-label="文件传输进度" max={Math.max(task.size, 1)} value={['queued', 'verifying'].includes(task.status) ? undefined : transferred} />
    <ol className="watch-stage-list" aria-label="上传阶段">{['校验', '传输', '保存与入库'].map((label, i) => { const step = task.status === 'saving' ? 2 : task.status === 'completed' ? 3 : transferred > 0 || task.status === 'uploading' ? 1 : 0; return <li key={label} data-state={task.status === 'completed' || i < step ? 'done' : i === step ? task.status === 'failed' ? 'error' : busy ? 'active' : 'waiting' : 'waiting'}>{label}</li>; })}</ol>
  </div>;
}
