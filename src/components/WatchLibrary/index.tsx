import { useEffect, useId, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { open } from '@tauri-apps/plugin-dialog';
import { stat } from '@tauri-apps/plugin-fs';
import { ArrowUpRight, ChevronLeft, ChevronRight, Download, FileText, ImagePlus, MessageSquarePlus, Pause, Pencil, Play, RefreshCw, Search, Upload, X } from 'lucide-react';
import { TaskProgress, UploadProgress } from './Progress';
import { useDispatch, useSelector } from 'react-redux';
import { sessionsActions, type RootState } from '../../store';
import { sessionsApi } from '../../services/tauri';
import type { MarketingUser } from '../../services/tauri/marketing';
import { watchLibraryApi as api, type WatchCandidate, type WatchProfile, type WatchStats, type WatchTask, type WatchUploadConfig, type WatchUploadRecord, type WatchUploadTask, type WatchVideo } from '../../services/tauri/watchLibrary';
import './WatchLibrary.css';

export interface WatchLibraryProps { active: boolean; user: MarketingUser | null; onNavigateToChat: () => void }
const PAGE_SIZE = 24;
const tabs = ['已拍摄内容', '型号检索', '共享档案', '上传素材'];
const eligible = (user: MarketingUser | null) => !!user && user.approvalStatus === 'active' && (user.accountType === 'marketing' || user.userRole === 'admin');
const identity = (user: MarketingUser | null) => `${user?.id}:${user?.userRole}:${user?.accountType}:${user?.approvalStatus}`;
const busyTask = (task?: WatchTask | null) => !!task && (['queued', 'running'].includes(task.status) || task.running === true);
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const bytes = (n = 0) => n >= 1024 ** 3 ? `${(n / 1024 ** 3).toFixed(1)} GB` : `${(n / 1024 ** 2).toFixed(1)} MB`;
const statuses: Record<string, string> = { queued: '排队中', running: '处理中', succeeded: '处理结束', failed: '失败', paused: '已暂停', uploading: '上传中', verifying: '校验中', saving: '正在保存 NAS', completed: '已完成', cancelled: '已取消', saved: '已保存 NAS', ready: '已研究', manual: '人工校正', partial: '资料不完整', visual_only: '仅外观匹配', description: '描述匹配', pending: '待核验', reviewed: '已复核', verified: '已核验' };
function safeUrl(url?: string) { try { const u = new URL(url || ''); return ['https:', 'http:'].includes(u.protocol) ? u.href : undefined; } catch { return undefined; } }
function useAlive() {
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  return alive;
}
function IconButton({ label, children, ...props }: { label: string; children: ReactNode } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button type="button" className="watch-icon" title={label} aria-label={label} {...props}>{children}</button>;
}
function Notice({ error, retry }: { error: string; retry?: () => void }) {
  return error ? <div className="watch-error" role="alert">{error}{retry && <button onClick={retry}>重试加载</button>}</div> : null;
}
function Cover({ id, name }: { id: number; name: string }) {
  const [url, setUrl] = useState('');
  const [loading, setLoading] = useState(true);
  useEffect(() => { let live = true; setUrl(''); setLoading(true); api.media(id, 'cover').then(r => { if (live) { const next = safeUrl(r.url) || ''; setUrl(next); if (!next) setLoading(false); } }).catch(() => { if (live) setLoading(false); }); return () => { live = false; }; }, [id]);
  return <div className="watch-cover" aria-busy={loading}>{loading && <div className="watch-skeleton-cover" aria-hidden="true" />}{url ? <img src={url} alt={name} loading="lazy" style={{ opacity: loading ? 0 : 1 }} onLoad={() => setLoading(false)} onError={() => { setUrl(''); setLoading(false); }} /> : !loading && <span>暂无封面</span>}</div>;
}
function StreamPlayer({url,expiresAt,onFailure,onRefresh}:{url:string;expiresAt?:number;onFailure:()=>void;onRefresh:()=>Promise<string>}) {
  const ref=useRef<HTMLVideoElement>(null);
  const failed=useRef(onFailure); failed.current=onFailure;
  const refresh=useRef(onRefresh); refresh.current=onRefresh;
  const resume=useRef<{time:number;paused:boolean}|null>(null);
  useEffect(()=>{
    const video=ref.current;if(!video)return;
    let disposed=false;let destroy:(()=>void)|undefined;
    const onMetadata=()=>{const saved=resume.current;if(saved){resume.current=null;video.currentTime=saved.time;if(!saved.paused)void video.play().catch(()=>{});}};
    video.addEventListener('loadedmetadata',onMetadata);
    if(!url.includes('/api/desktop/hls/') || video.canPlayType('application/vnd.apple.mpegurl')) {
      video.src=url;
    } else {
      void import('hls.js').then(({default:Hls})=>{
        if(disposed)return;
        if(!Hls.isSupported()){failed.current();return;}
        const hls=new Hls(); destroy=()=>hls.destroy();
        hls.on(Hls.Events.ERROR,(_event,data)=>{if(data.fatal)failed.current();});
        hls.loadSource(url);hls.attachMedia(video);
      }).catch(()=>{if(!disposed)failed.current();});
    }
    const timer=expiresAt ? setTimeout(()=>{resume.current={time:video.currentTime,paused:video.paused};void refresh.current().catch(()=>{if(!disposed)failed.current();});},Math.max(1000,expiresAt*1000-Date.now()-30000)) : undefined;
    return ()=>{disposed=true;clearTimeout(timer);video.removeEventListener('loadedmetadata',onMetadata);destroy?.();video.removeAttribute('src');video.load();};
  },[url,expiresAt]);
  return <video ref={ref} controls autoPlay onError={()=>failed.current()} />;
}
function Profile({ profile }: { profile: WatchProfile }) {
  const facts: Array<[string, unknown]> = [['品牌', profile.brand_cn || profile.brand], ['系列', profile.series_cn || profile.series], ['完整型号', profile.model], ['材质', profile.material], ['表盘', profile.dial], ['表带', profile.bracelet], ['表圈', profile.bezel], ['尺寸 (mm)', profile.case_size_mm], ['状态', statuses[profile.status || ''] || profile.status], ['更新时间', profile.updated_at]];
  return <section className="watch-profile"><h3>{profile.model || '腕表研究档案'}</h3>{profile.description && <p>{profile.description}</p>}
    <dl>{facts.filter(([, v]) => v != null && v !== '').map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{String(v)}</dd></div>)}</dl>
    <p>{profile.tags?.join(' · ')}</p>{[...(profile.warnings || []), ...(profile.uncertainties || [])].map((w, i) => <p className="watch-warning" key={i}>{w}</p>)}
    <h4>可追溯来源</h4>{!profile.sources?.length && <p className="watch-muted">暂无来源，信息待核验</p>}
    <ol>{profile.sources?.map((raw, i) => { const s = typeof raw === 'string' ? { url: raw } : raw; const url = safeUrl(s.url || s.page_url || s.source_url); return <li key={i}>{s.type === 'manual' ? `人工校正：${s.reviewed_by || ''} ${s.reviewed_at || ''}` : url ? <a href={url} target="_blank" rel="noreferrer">{s.title || url}</a> : (s.title || '来源待核验')}{s.snippet && <p>{s.snippet}</p>}</li>; })}</ol>
    {profile.field_evidence && <details><summary>字段依据</summary><pre>{JSON.stringify(profile.field_evidence, null, 2)}</pre></details>}
  </section>;
}
function Task({ task }: { task: WatchTask }) {
  return <section className="watch-task" aria-label="任务进度"><TaskProgress task={task} />
    {task.error && <Notice error={task.error} />}{task.result?.research_complete === false && <p className="watch-warning">研究尚未完整，请核验来源</p>}
    {[...(task.result?.warnings || []), ...(task.result?.errors || [])].map((w, i) => <p className="watch-warning" key={i}>{w}</p>)}
    {task.result?.trace?.length ? <details open><summary>检索过程与证据链</summary><ol>{task.result.trace.map((r, i) => <li key={i}>{typeof r === 'string' ? r : <>{r.stage} · {r.status} · {r.message} {safeUrl(r.url) && <a href={safeUrl(r.url)} target="_blank" rel="noreferrer">查看来源</a>}</>}</li>)}</ol></details> : null}
  </section>;
}
function LoadingCards({ profiles = false }: { profiles?: boolean }) {
  return <><span className="watch-loading-label" role="status">{profiles ? '正在加载档案…' : '正在加载资料…'}</span>{Array.from({ length: 6 }, (_, i) => <div key={i} className="watch-skeleton" aria-hidden="true">{!profiles && <div className="watch-skeleton-cover" />}<div className="watch-skeleton-lines"><i /><i /><i /></div></div>)}</>;
}
function ProfileCard({ profile: p, order, onOpen }: { profile: WatchProfile; order: number; onOpen: () => void }) {
  return <button className="watch-profile-card watch-card-enter" style={{ '--watch-order': Math.min(order, 8) } as CSSProperties} onClick={onOpen} disabled={!p.id}>
    <div className="watch-profile-footer"><span><FileText size={16} /> {p.brand_cn || p.brand || '品牌待核验'}</span><ArrowUpRight size={16} /></div>
    <strong>{p.model || '型号待核验'}</strong><span>{p.series_cn || p.series || '系列待核验'}</span>
    <div className="watch-profile-meta">{[p.material, p.dial, p.case_size_mm ? `${p.case_size_mm} mm` : ''].filter(Boolean).join(' · ') || '规格待补充'}</div>
    {p.description && <p className="watch-profile-excerpt">{p.description}</p>}
    {!!p.tags?.length && <div className="watch-profile-tags">{p.tags.slice(0, 3).map((tag, i) => <span key={i}>{tag}</span>)}</div>}
    <div className="watch-profile-footer"><small>{statuses[p.status || ''] || p.status || '待核验'}</small><small>{p.sources?.length || 0} 条来源</small></div>
  </button>;
}
function useTask(id: string, onComplete?: () => void) {
  const [task, setTask] = useState<WatchTask | null>(null);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const complete = useRef(onComplete); complete.current = onComplete;
  useEffect(() => {
    if (!id) { setTask(null); setError(''); return; }
    let live = true; let timer: ReturnType<typeof setTimeout>;
    setTask(null); setError('');
    const poll = async () => {
      try { const next = await api.request<WatchTask>({ resource: 'task', id }); if (!live) return; setTask(next); if (busyTask(next)) timer = setTimeout(poll, 1600); else complete.current?.(); }
      catch (e) { if (live) setError(errorText(e)); }
    };
    void poll(); return () => { live = false; clearTimeout(timer); };
  }, [id, revision]);
  return { task, error, retry: () => setRevision(n => n + 1) };
}

export function WatchLibrary(props: WatchLibraryProps) {
  if (!props.active) return null;
  if (!eligible(props.user)) return <section className="watch-library"><p>{props.user ? '当前账户暂无腕表资料库访问权限' : '请先登录营销账户'}</p></section>;
  return <Dashboard key={identity(props.user)} {...props} user={props.user!} />;
}
export default WatchLibrary;

function Dashboard({ user, onNavigateToChat }: WatchLibraryProps & { user: MarketingUser }) {
  const [tab, setTab] = useState(0);
  const [refresh, setRefresh] = useState(0);
  const [stats, setStats] = useState<WatchStats>({});
  const [series, setSeries] = useState<Array<{ series_cn: string; n: number }>>([]);
  const [brand, setBrand] = useState(''); const [seriesName, setSeriesName] = useState('');
  const [query, setQuery] = useState(''); const [search, setSearch] = useState(''); const [page, setPage] = useState(0);
  const [videos, setVideos] = useState<WatchVideo[]>([]); const [profiles, setProfiles] = useState<WatchProfile[]>([]);
  const [loading, setLoading] = useState(false); const [error, setError] = useState('');
  const [selected, setSelected] = useState<number | null>(null); const [profileId, setProfileId] = useState('');
  const [jobId, setJobId] = useState(''); const [jobBusy, setJobBusy] = useState(false);
  const alive = useAlive();
  const job = useTask(jobId, () => setRefresh(n => n + 1));
  useEffect(() => { let live = true; api.request<WatchStats>({ resource: 'stats' }).then(r => { if (live) setStats(r); }).catch(e => { if (live) setError(errorText(e)); }); return () => { live = false; }; }, [refresh]);
  useEffect(() => { let live = true; setSeries([]); api.request<typeof series>({ resource: 'series', query: { brand } }).then(r => { if (live) setSeries(r); }).catch(e => { if (live) setError(errorText(e)); }); return () => { live = false; }; }, [brand, refresh]);
  useEffect(() => {
    if (tab !== 0 && tab !== 2) return;
    let live = true; setLoading(true); setError(''); setVideos([]); setProfiles([]);
    const request = tab === 0 ? api.request<WatchVideo[]>({ resource: 'videos', query: { brand, series: seriesName, q: search, offset: String(page * PAGE_SIZE), limit: String(PAGE_SIZE) } }) : api.request<WatchProfile[]>({ resource: 'profiles', query: { query: search } });
    request.then(r => { if (live) { if (tab === 0) setVideos(r as WatchVideo[]); else setProfiles(r as WatchProfile[]); } }).catch(e => { if (live) setError(errorText(e)); }).finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [tab, brand, seriesName, search, page, refresh]);
  useEffect(() => {
    if (user.userRole !== 'admin') return;
    let live = true; let timer: ReturnType<typeof setTimeout>;
    const poll = async () => { try { const results = await Promise.all([api.request<WatchTask>({ resource: 'scan-status' }), api.request<WatchTask>({ resource: 'ai-status' })]); if (!live) return; const running = results.find(busyTask); if (running?.id) setJobId(running.id); timer = setTimeout(poll, 5000); } catch (e) { if (live) setError(errorText(e)); } };
    void poll(); return () => { live = false; clearTimeout(timer); };
  }, [user.userRole, refresh]);
  async function startJob(resource: 'scan' | 'analyze', force = false) {
    setJobBusy(true); setError('');
    try { const r = await api.request<{ task_id: string }>({ resource, ...(resource === 'analyze' ? { query: { force: String(force) } } : {}) }); if (!r.task_id) throw new Error('服务器未返回任务编号'); if (alive.current) setJobId(r.task_id); }
    catch (e) { if (alive.current) setError(errorText(e)); } finally { if (alive.current) setJobBusy(false); }
  }
  return <section className={`watch-library${profileId ? ' has-profile-modal' : ''}`}>
    <header className="watch-heading"><div><span className="watch-muted">HARMONY WATCH LIBRARY</span><h1>腕表如诗内容看板</h1></div><div className="watch-actions"><button onClick={() => window.dispatchEvent(new Event('harmony:watch-uploads-open'))}><Upload size={16} />上传任务</button><IconButton label="刷新" onClick={() => setRefresh(n => n + 1)}><RefreshCw size={16} /></IconButton></div></header>
    <div role="tablist" aria-label="腕表资料视图" className="watch-tabs">{tabs.map((name, i) => <button role="tab" aria-selected={tab === i} key={name} onClick={() => { setTab(i); setPage(0); setQuery(''); setSearch(''); setSelected(null); setProfileId(''); }}>{name}</button>)}</div>
    <div className="watch-content"><Notice error={error} retry={() => setRefresh(n => n + 1)} />
      {(tab === 0 || tab === 2) && <form className="watch-filters" onSubmit={e => { e.preventDefault(); setSearch(query.trim()); setPage(0); }}>
        {tab === 0 && <><select aria-label="品牌" value={brand} onChange={e => { setBrand(e.target.value); setSeriesName(''); setPage(0); }}><option value="">全部品牌</option>{stats.brands?.map(b => <option key={b.brand} value={b.brand}>{b.brand || '未分类'} ({b.n})</option>)}</select><select aria-label="系列" value={seriesName} onChange={e => { setSeriesName(e.target.value); setPage(0); }}><option value="">全部系列</option>{series.map((s, i) => <option key={i} value={s.series_cn}>{s.series_cn} ({s.n})</option>)}</select></>}
        <input aria-label="搜索资料" placeholder={tab === 0 ? '搜索型号、系列或文件名' : '搜索共享档案'} value={query} onChange={e => setQuery(e.target.value)} /><button type="submit" aria-label="搜索" title="搜索"><Search size={16} /></button>
      </form>}
      {tab === 0 && <><div className="watch-stats"><span>拍摄素材 <strong>{stats.count ?? '—'}</strong></span><span>已分析 <strong>{stats.ai ?? '—'}</strong></span><span>资料容量 <strong>{stats.total_bytes == null ? '—' : bytes(stats.total_bytes)}</strong></span></div>
        <div className={`watch-catalog${selected !== null ? ' has-detail' : ''}`}><div><div className="watch-grid" aria-busy={loading}>{loading ? <LoadingCards /> : !videos.length ? <p className="watch-empty">没有找到素材</p> : videos.map((v, i) => <button key={v.id} style={{ '--watch-order': Math.min(i, 8) } as CSSProperties} className={`watch-asset watch-card-enter${selected === v.id ? ' selected' : ''}`} onClick={() => { setSelected(v.id); setProfileId(''); }}><Cover id={v.id} name={v.filename} /><div className="watch-asset-info"><strong>{v.brand} · {v.series_cn || v.series || '系列待核验'}</strong><span>{v.filename}</span><small>{bytes(v.size)} · {v.duration || 0} 秒 · {v.analyzed_at ? '已分析' : '未分析'}</small></div></button>)}</div><div className="watch-pagination"><span>第 {page + 1} 页 · 本页 {videos.length} 条</span><IconButton label="上一页" disabled={loading || page === 0} onClick={() => setPage(n => n - 1)}><ChevronLeft size={16} /></IconButton><IconButton label="下一页" disabled={loading || videos.length < PAGE_SIZE} onClick={() => setPage(n => n + 1)}><ChevronRight size={16} /></IconButton></div></div>
          {selected !== null && <AssetDetail key={selected} id={selected} user={user} onClose={() => setSelected(null)} onProfile={setProfileId} onNavigateToChat={onNavigateToChat} onChanged={() => setRefresh(n => n + 1)} />}</div></>}
      {tab === 1 && <Research onAsset={setSelected} />}
      {(tab === 1 || tab === 3) && selected !== null && <AssetDetail key={selected} id={selected} user={user} onClose={() => setSelected(null)} onProfile={setProfileId} onNavigateToChat={onNavigateToChat} onChanged={() => setRefresh(n => n + 1)} />}
      {tab === 2 && <div className="watch-grid" aria-busy={loading}>{loading ? <LoadingCards profiles /> : !profiles.length ? <p className="watch-empty">暂无共享档案</p> : profiles.map((p, i) => <ProfileCard key={p.id || i} profile={p} order={i} onOpen={() => setProfileId(p.id || '')} />)}</div>}
      {tab === 3 && <Uploads onAsset={setSelected} />}
      {profileId && <ProfileDetail key={profileId} id={profileId} onClose={() => setProfileId('')} />}
      {user.userRole === 'admin' && <details className="watch-admin"><summary>资料管理</summary><div className="watch-actions"><button disabled={jobBusy || busyTask(job.task)} onClick={() => void startJob('scan')}>扫描资料</button><button disabled={jobBusy || busyTask(job.task)} onClick={() => void startJob('analyze')}>分析新增资料</button><button disabled={jobBusy || busyTask(job.task)} onClick={() => { if (window.confirm('确认重新分析全部资料？')) void startJob('analyze', true); }}>强制重新分析</button></div><Notice error={job.error} retry={job.retry} />{job.task && <Task task={job.task} />}</details>}
    </div>
  </section>;
}

function ProfileDetail({ id, onClose }: { id: string; onClose: () => void }) {
  const [profile, setProfile] = useState<WatchProfile | null>(null); const [error, setError] = useState(''); const [revision, setRevision] = useState(0);
  const dialog = useRef<HTMLDialogElement>(null); const title = useId();
  const [closing, setClosing] = useState(false); const closeTimer = useRef<ReturnType<typeof setTimeout>>();
  const close = () => { if (closing) return; setClosing(true); closeTimer.current = setTimeout(onClose, 160); };
  useEffect(() => {
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const node = dialog.current; node?.showModal();
    return () => { clearTimeout(closeTimer.current); node?.close(); if (trigger?.isConnected) trigger.focus({ preventScroll: true }); };
  }, []);
  useEffect(() => { let live = true; setError(''); api.request<WatchProfile>({ resource: 'profile', id }).then(r => { if (live) setProfile(r); }).catch(e => { if (live) setError(errorText(e)); }); return () => { live = false; }; }, [id, revision]);
  return createPortal(<dialog ref={dialog} className="watch-profile-modal watch-library" aria-labelledby={title} data-closing={closing || undefined} onCancel={e => { e.preventDefault(); close(); }} onClick={e => { const box = e.currentTarget.getBoundingClientRect(); if (e.target === e.currentTarget && (e.clientX < box.left || e.clientX > box.right || e.clientY < box.top || e.clientY > box.bottom)) close(); }}>
    <header className="watch-modal-header"><div><span className="watch-muted">腕表如诗 · 共享资料</span><h2 id={title}>完整腕表档案</h2></div><IconButton label="关闭档案" onClick={close}><X size={18} /></IconButton></header>
    <div className="watch-modal-body" aria-busy={!profile && !error}><Notice error={error} retry={() => setRevision(n => n + 1)} />{profile ? <Profile profile={profile} /> : !error && <p role="status">正在加载档案…</p>}</div>
  </dialog>, document.body);
}

function AssetDetail({ id, user, onClose, onProfile, onNavigateToChat, onChanged }: { id: number; user: MarketingUser; onClose: () => void; onProfile: (id: string) => void; onNavigateToChat: () => void; onChanged: () => void }) {
  const [asset, setAsset] = useState<WatchVideo | null>(null); const [error, setError] = useState(''); const [notice, setNotice] = useState(''); const [busy, setBusy] = useState(false); const [play, setPlay] = useState<{url:string;expiresAt?:number}|null>(null); const [editing, setEditing] = useState(false); const [revision, setRevision] = useState(0);
  const dispatch = useDispatch(); const alive = useAlive();
  const activeSessionId = useSelector((s: RootState) => s.sessions.activeSessionId);
  const settings = useSelector((s: RootState) => s.settings.settings);
  useEffect(() => { let live = true; setError(''); api.request<WatchVideo>({ resource: 'video', id: String(id) }).then(r => { if (live) setAsset(r); }).catch(e => { if (live) setError(errorText(e)); }); return () => { live = false; }; }, [id, revision]);
  async function action(kind: 'play' | 'save' | 'chat') {
    setBusy(true); setError(''); setNotice('');
    try {
      if (kind === 'play') {
        const media = await api.media(id, 'play'); if (!alive.current) return;
        const url = safeUrl(media.url); if (!url) throw new Error('播放地址无效');
        setPlay({url,expiresAt:media.expiresAt}); return;
      }
      if (kind === 'save' && !settings?.workspace_root) throw new Error('请先配置工作区目录');
      const prepared = await api.prepareAsset(id, settings?.workspace_root || undefined); if (!alive.current) return;
      if (!prepared.localPath || !prepared.fileName || !prepared.mediaType) throw new Error('原生附件接口未返回有效文件');
      if (kind === 'save') { setNotice(`已保存到工作区：${prepared.localPath}`); return; }
      const session = activeSessionId ? null : await sessionsApi.create('腕表素材分析', 'chat'); if (!alive.current) return;
      if (session) dispatch(sessionsActions.addSession(session));
      const target = activeSessionId || session?.id; if (!target) throw new Error('无法创建会话');
      dispatch(sessionsActions.openMainChatView({ filter: 'chat', sessionId: target, composerDraft: `请基于腕表素材分析：${asset?.filename || prepared.fileName}\n本地文件：${prepared.localPath}\n我的具体需求是：`, composerAttachments: [{ path: prepared.localPath, media_type: prepared.mediaType, filename: prepared.fileName }], autoSend: false }));
      onNavigateToChat();
    } catch (e) { if (alive.current) setError(errorText(e)); } finally { if (alive.current) setBusy(false); }
  }
  return <aside className="watch-detail" aria-label="素材详情"><div className="watch-actions"><h2>素材详情</h2><IconButton label="关闭详情" onClick={onClose}><X size={16} /></IconButton></div><Notice error={error} retry={() => setRevision(n => n + 1)} />
    {!asset ? !error && <p role="status">正在加载详情…</p> : <><Cover id={id} name={asset.filename} />{play && (/\.(jpe?g|png|webp)$/i.test(asset.filename) ? <img src={play.url} alt={asset.filename} style={{maxWidth:'100%',maxHeight:'60vh',objectFit:'contain'}} onError={()=>{setPlay(null);setError('图片预览失败，请重试');}} /> : <StreamPlayer url={play.url} expiresAt={play.expiresAt} onRefresh={async()=>{const media=await api.media(id,'play');const url=safeUrl(media.url);if(!url)throw new Error('播放地址无效');if(alive.current)setPlay({url,expiresAt:media.expiresAt});return url;}} onFailure={() => { setPlay(null); setError('视频无法播放，可能是地址已过期或当前客户端不支持该格式，请重试或保存原文件'); }} />)}
      <h3>{asset.brand} · {asset.series_cn || asset.series}</h3><p>{asset.filename}</p><p className="watch-muted">{bytes(asset.size)} · {asset.duration || 0} 秒 · {asset.width || '—'} × {asset.height || '—'}</p>
      <div className="watch-actions"><IconButton label={/\.(jpe?g|png|webp)$/i.test(asset.filename) ? '查看原图' : '播放'} disabled={busy} onClick={() => void action('play')}><Play size={16} /></IconButton><button disabled={busy} onClick={() => void action('chat')}><MessageSquarePlus size={16} />放入会话</button><IconButton label="保存到工作区" disabled={busy} onClick={() => void action('save')}><Download size={16} /></IconButton>{user.userRole === 'admin' && <IconButton label="编辑资料" onClick={() => setEditing(!editing)}><Pencil size={16} /></IconButton>}</div>{busy && <p role="status">正在准备文件…</p>}{notice && <p role="status">{notice}</p>}
      {editing && <EditAsset asset={asset} onSaved={r => { setAsset(r); setEditing(false); onChanged(); }} />}
      {asset.profile ? <><h4>关联研究档案</h4><Profile profile={asset.profile} />{(asset.profile.id || asset.profile_id) && <button onClick={() => onProfile(asset.profile?.id || asset.profile_id || '')}>查看完整档案</button>}</> : asset.profile_id ? <button onClick={() => onProfile(asset.profile_id!)}>查看关联档案</button> : asset.ai ? <Profile profile={asset.ai} /> : <p className="watch-muted">暂无关联档案</p>}
    </>}
  </aside>;
}

function EditAsset({ asset, onSaved }: { asset: WatchVideo; onSaved: (asset: WatchVideo) => void }) {
  const p = { ...asset.ai, ...asset.profile }; const [error, setError] = useState(''); const [busy, setBusy] = useState(false); const alive = useAlive();
  const fields: Array<[string, string, string | number | undefined]> = [['brand', '品牌', asset.brand || p.brand], ['series', '系列', asset.series_cn || asset.series || p.series], ['model', '完整型号', p.model || asset.ref || asset.model_code], ['material', '材质', p.material], ['dial', '表盘', p.dial], ['bracelet', '表带', p.bracelet], ['bezel', '表圈', p.bezel], ['case_size_mm', '尺寸 (mm)', p.case_size_mm], ['tags', '标签', p.tags?.join('，')]];
  return <form className="watch-edit" onSubmit={async e => { e.preventDefault(); const body = Object.fromEntries(new FormData(e.currentTarget)); setBusy(true); setError(''); try { const result = await api.request<WatchVideo>({ resource: 'edit', id: String(asset.id), body: { ...body, tags: String(body.tags).split(/[,，;；\n]/).map(t => t.trim()).filter(Boolean), case_size_mm: body.case_size_mm ? Number(body.case_size_mm) : null } }); if (alive.current) onSaved(result); } catch (err) { if (alive.current) setError(errorText(err)); } finally { if (alive.current) setBusy(false); } }}>
    {fields.map(([name, label, value]) => <label key={name}>{label}<input name={name} defaultValue={value ?? ''} required={name === 'brand' || name === 'series'} maxLength={name === 'tags' ? 1000 : name === 'series' ? 150 : name === 'brand' || name === 'model' ? 100 : 200} type={name === 'case_size_mm' ? 'number' : 'text'} min={name === 'case_size_mm' ? 0.1 : undefined} max={name === 'case_size_mm' ? 100 : undefined} step={name === 'case_size_mm' ? 0.1 : undefined} /></label>)}<button disabled={busy} type="submit">{busy ? '正在保存…' : '保存校正'}</button><Notice error={error} />
  </form>;
}

function Research({ onAsset }: { onAsset: (id: number) => void }) {
  const [query, setQuery] = useState(''); const [id, setId] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false); const alive = useAlive(); const task = useTask(id);
  async function start(image: boolean) {
    setBusy(true); setError(''); setId('');
    try {
      let result: { task_id: string };
      if (image) {
        const filePath = await open({ multiple: false, filters: [{ name: '图片', extensions: ['jpg', 'jpeg', 'png', 'webp'] }] }); if (!alive.current || !filePath || Array.isArray(filePath)) return;
        const info = await stat(filePath); if (!alive.current) return;
        if (info.size > 8 * 1024 * 1024) throw new Error('图片不能超过 8 MB');
        result = await api.imageQuery(filePath);
      } else { if (!query.trim()) throw new Error('请输入型号或描述'); result = await api.request({ resource: 'match-text', body: { query: query.trim() } }); }
      if (!result.task_id) throw new Error('服务器未返回检索任务编号'); if (alive.current) setId(result.task_id);
    } catch (e) { if (alive.current) setError(errorText(e)); } finally { if (alive.current) setBusy(false); }
  }
  const running = busy || (!!id && !task.task && !task.error) || busyTask(task.task);
  const result = task.task?.result;
  const candidates = result?.results?.length ? result.results : result?.best ? [result.best] : [];
  return <section className="watch-research"><form className="watch-filters" onSubmit={e => { e.preventDefault(); void start(false); }}><input aria-label="型号或描述" maxLength={500} value={query} onChange={e => setQuery(e.target.value)} placeholder="输入完整型号或腕表描述" /><button disabled={running} type="submit"><Search size={16} />开始检索</button><button type="button" disabled={running} onClick={() => void start(true)}><ImagePlus size={16} />图片查询</button></form>
    <Notice error={error} /><Notice error={task.error} retry={task.retry} />{running && !task.task && <TaskProgress task={{ id: id || 'submitting', status: 'queued', stage: busy ? '正在提交检索请求' : '请求已提交，等待研究任务' }} />}{task.task && <Task task={task.task} />}
    {result && <><h2>{result.verdict || '信息不足，需核验'}</h2>{(result.profile || result.parsed) && <Profile profile={(result.profile || result.parsed)!} />}{candidates.map((c: WatchCandidate, i) => { const asset = typeof c.video === 'object' ? { ...c.video, ...c } : c; const assetId = asset.id ?? asset.video_id; return <article className="watch-candidate" key={i}>{assetId != null && <Cover id={assetId} name={asset.filename || '候选素材'} />}<div><h3>{asset.filename || (typeof c.video === 'string' ? c.video : '候选素材')}</h3><p>{c.verdict || c.match_type}</p><ul>{c.reasons?.map((r, j) => <li key={j}>{r}</li>)}</ul><ul className="watch-warning">{c.differences?.map((d, j) => <li key={j}>{d}</li>)}</ul>{assetId != null && <button onClick={() => onAsset(assetId)}>查看素材与播放</button>}</div></article>; })}{!candidates.length && <p className="watch-empty">暂未找到有可靠依据的拍摄素材</p>}</>}
  </section>;
}

function Uploads({ onAsset }: { onAsset: (id: number) => void }) {
  const [config, setConfig] = useState<WatchUploadConfig | null>(null); const [rows, setRows] = useState<WatchUploadRecord[]>([]); const [folder, setFolder] = useState(''); const [selected, setSelected] = useState<string[]>([]); const [error, setError] = useState(''); const [loading, setLoading] = useState(true); const [busy, setBusy] = useState(false); const [revision, setRevision] = useState(0); const [taskId, setTaskId] = useState(''); const alive = useAlive(); const task = useTask(taskId, () => setRevision(n => n + 1));
  useEffect(() => {
    let live = true; let timer: ReturnType<typeof setTimeout>; setLoading(true); setError('');
    const load = async () => { try { const [c, r] = await Promise.all([api.request<WatchUploadConfig>({ resource: 'uploads-config' }), api.request<WatchUploadRecord[]>({ resource: 'uploads' })]); if (!live) return; setConfig(c); setFolder(old => c.folders.includes(old) ? old : c.folders[0] || ''); setRows(r); setSelected(old => old.filter(id => r.some(row => row.id === id && row.asset_id && !busyTask(row.analysis_task)))); setError(''); } catch (e) { if (live) setError(errorText(e)); } finally { if (live) { setLoading(false); timer = setTimeout(load, 3000); } } };
    void load(); return () => { live = false; clearTimeout(timer); };
  }, [revision]);
  async function upload() {
    setBusy(true); setError('');
    try { const paths = await open({ multiple: true, filters: config?.extensions.length ? [{ name: '素材', extensions: config.extensions.map(e => e.replace(/^\./, '')) }] : undefined }); if (!alive.current || !paths) return; const files = Array.isArray(paths) ? paths : [paths]; for (const path of files) { const info = await stat(path); if (!alive.current) return; if (info.size > (config?.max_file_bytes || 0)) throw new Error('文件超过上传大小限制'); } await api.uploadStart(files, folder); if (!alive.current) return; window.dispatchEvent(new Event('harmony:watch-uploads-open')); setRevision(n => n + 1); }
    catch (e) { if (alive.current) setError(errorText(e)); } finally { if (alive.current) setBusy(false); }
  }
  async function analyze(ids: string[]) {
    setBusy(true); setError('');
    try { const r = await api.request<{ task_id: string }>({ resource: 'upload-analyze', body: { ids } }); if (!r.task_id) throw new Error('服务器未返回分析任务编号'); if (alive.current) { setTaskId(r.task_id); setSelected([]); setRevision(n => n + 1); } } catch (e) { if (alive.current) setError(errorText(e)); } finally { if (alive.current) setBusy(false); }
  }
  return <section><div className="watch-filters"><select aria-label="上传目录" value={folder} onChange={e => setFolder(e.target.value)}>{config?.folders.map(f => <option key={f}>{f}</option>)}</select><button disabled={!config?.enabled || !folder || busy} onClick={() => void upload()}><Upload size={16} />选择并上传素材</button><button disabled={!selected.length || busy || busyTask(task.task)} onClick={() => void analyze(selected)}>分析所选素材</button><IconButton label="刷新上传记录" onClick={() => setRevision(n => n + 1)}><RefreshCw size={16} /></IconButton></div>{config && <p className="watch-muted">{config.enabled ? `${config.extensions.join(' / ')} · 单文件上限 ${bytes(config.max_file_bytes)}` : '上传服务未启用'}</p>}<Notice error={error} retry={() => setRevision(n => n + 1)} />{loading && <p role="status">正在加载上传记录…</p>}<Notice error={task.error} retry={task.retry} />{task.task && <Task task={task.task} />}
    <div className="watch-table"><table><caption>我的上传记录</caption>
      <thead><tr><th>选择</th><th>文件</th><th>保存状态</th><th>分析状态</th><th>操作</th></tr></thead>
      <tbody>{rows.map(r => <tr key={r.id}>
        <td><input type="checkbox" aria-label={`选择 ${r.filename}`} checked={selected.includes(r.id)} disabled={!r.asset_id || busyTask(r.analysis_task)} onChange={e => setSelected(old => e.target.checked ? [...old, r.id] : old.filter(id => id !== r.id))} /></td>
        <td>{r.filename}<small>{r.destination || r.folder} · {bytes(r.size)} · {r.created_at}</small></td>
        <td>{statuses[r.status] || r.status}{r.status === 'saved' && !r.asset_id && ' · 等待入库'}{r.index_task?.error && <Notice error={r.index_task.error} />}</td>
        <td>{r.analysis_task ? statuses[r.analysis_task.status] || r.analysis_task.status : r.asset?.analyzed_at ? '已分析' : '未分析'}{r.analysis_task?.error && <Notice error={r.analysis_task.error} />}</td>
        <td><div className="watch-actions"><button disabled={!r.asset_id || busy || busyTask(r.analysis_task)} onClick={() => void analyze([r.id])}>开始分析</button>{r.asset_id && <button onClick={() => onAsset(r.asset_id!)}>查看入库素材</button>}</div>{r.asset?.profile && <details><summary>查看结果</summary><Profile profile={r.asset.profile} /></details>}</td>
      </tr>)}</tbody></table></div>{!loading && !rows.length && <p className="watch-empty">还没有上传记录</p>}
  </section>;
}

export function WatchUploadWindow({ user }: { user: MarketingUser | null }) {
  return eligible(user) ? <UploadWindow key={identity(user)} /> : null;
}
function UploadWindow() {
  const [opened, setOpened] = useState(false); const [rows, setRows] = useState<WatchUploadTask[]>([]); const [error, setError] = useState(''); const [loading, setLoading] = useState(true); const [revision, setRevision] = useState(0); const [busy, setBusy] = useState(''); const alive = useAlive(); const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { const show = () => setOpened(true); window.addEventListener('harmony:watch-uploads-open', show); return () => window.removeEventListener('harmony:watch-uploads-open', show); }, []);
  useEffect(() => { if (opened) dialog.current?.showModal(); else dialog.current?.close(); }, [opened]);
  useEffect(() => {
    let live = true; let timer: ReturnType<typeof setTimeout>;
    const poll = async () => { try { const r = await api.uploads(); if (live) { setRows(r); setError(''); } } catch (e) { if (live) setError(errorText(e)); } finally { if (live) { setLoading(false); timer = setTimeout(poll, 1500); } } };
    void poll(); return () => { live = false; clearTimeout(timer); };
  }, [revision]);
  async function action(id: string, action: 'pause' | 'resume' | 'cancel' | 'retry') {
    setBusy(id); setError(''); try { await api.uploadAction(id, action); if (alive.current) setRevision(n => n + 1); } catch (e) { if (alive.current) setError(errorText(e)); } finally { if (alive.current) setBusy(''); }
  }
  return <dialog ref={dialog} className="watch-upload-dialog watch-library" aria-label="上传任务" onCancel={() => setOpened(false)}><header className="watch-heading"><h2>上传任务</h2><IconButton label="关闭上传任务" onClick={() => setOpened(false)}><X size={16} /></IconButton></header><div className="watch-content"><p className="watch-upload-summary">{rows.filter(r => r.status === 'completed').length} 已完成 · {rows.filter(r => ['queued', 'uploading', 'verifying', 'saving'].includes(r.status)).length} 进行中 · {rows.filter(r => r.status === 'failed').length} 失败</p><Notice error={error} retry={() => { setError(''); setRevision(n => n + 1); }} />{loading && <p role="status">正在加载上传任务…</p>}{!loading && !rows.length && <p className="watch-empty">暂无上传任务</p>}{rows.map(r => <article className="watch-upload-item" key={r.id}><strong>{r.fileName}</strong><p className="watch-muted">{r.source === "marketing" ? "素材库" : "腕表看板"} · {r.folder}</p><UploadProgress task={r} /><p>{bytes(r.uploaded)} / {bytes(r.size)}{r.bytesPerSecond != null && r.status === 'uploading' ? ` · ${bytes(r.bytesPerSecond)}/s` : ''}</p>{r.error && <Notice error={r.error} />}<div className="watch-actions">{['queued', 'verifying', 'uploading'].includes(r.status) && <IconButton label="暂停" disabled={busy === r.id} onClick={() => void action(r.id, 'pause')}><Pause size={16} /></IconButton>}{r.status === 'paused' && <IconButton label="继续" disabled={busy === r.id} onClick={() => void action(r.id, 'resume')}><Play size={16} /></IconButton>}{r.status === 'failed' && <IconButton label="重试" disabled={busy === r.id} onClick={() => void action(r.id, 'retry')}><RefreshCw size={16} /></IconButton>}{['queued', 'verifying', 'uploading', 'paused', 'failed'].includes(r.status) && <IconButton label="取消上传" disabled={busy === r.id} onClick={() => void action(r.id, 'cancel')}><X size={16} /></IconButton>}</div></article>)}</div></dialog>;
}
