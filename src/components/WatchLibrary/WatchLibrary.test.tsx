import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Provider } from 'react-redux';
import { configureStore } from '@reduxjs/toolkit';
import { WatchLibrary, WatchUploadWindow } from './index';
import type { MarketingUser } from '../../services/tauri/marketing';
import { watchLibraryApi } from '../../services/tauri/watchLibrary';
import { open } from '@tauri-apps/plugin-dialog';
import { stat } from '@tauri-apps/plugin-fs';
import { sessionsApi } from '../../services/tauri';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-fs', () => ({ stat: vi.fn() }));
vi.mock('../../services/tauri', () => ({ sessionsApi: { create: vi.fn() } }));
vi.mock('../../services/tauri/watchLibrary', () => ({ watchLibraryApi: { request: vi.fn(), media: vi.fn(), imageQuery: vi.fn(), prepareAsset: vi.fn(), uploads: vi.fn(), uploadAction: vi.fn(), uploadStart: vi.fn() } }));
const user: MarketingUser = { id: 'one', name: '甲', email: 'one@test.cn', userRole: 'user', accountType: 'marketing', approvalStatus: 'active' };
const store = configureStore({ reducer: () => ({ sessions: { activeSessionId: null }, settings: { settings: { workspace_root: '/work' } } }) });
const mount = (u: MarketingUser | null = user, active = true) => <Provider store={store}><WatchLibrary active={active} user={u} onNavigateToChat={vi.fn()} /></Provider>;
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => {
    if (resource === 'stats') return { count: 1, ai: 1, brands: [{ brand: '伯爵', n: 1 }] } as never;
    if (resource === 'uploads-config') return { enabled: true, folders: ['伯爵'], extensions: ['.mp4'], max_file_bytes: 100 } as never;
    return [] as never;
  });
  vi.mocked(watchLibraryApi.media).mockResolvedValue({ url: 'https://example.com/cover.jpg' });
  vi.mocked(watchLibraryApi.uploads).mockResolvedValue([]);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });
it('opens shared profiles in a modal and restores focus on close', async () => {
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => resource === 'profiles' ? [{ id: 'p1', model: 'G0A123', material: '精钢', sources: ['https://example.com'] }] as never : resource === 'profile' ? { id: 'p1', model: 'G0A123', description: '完整档案内容' } as never : [] as never);
  render(mount());
  fireEvent.click(screen.getByRole('tab', { name: '共享档案' }));
  const card = await screen.findByRole('button', { name: /G0A123/ });
  card.focus(); fireEvent.click(card);
  const modal = await screen.findByRole('dialog', { name: '完整腕表档案' });
  expect(await within(modal).findByText('完整档案内容')).toBeInTheDocument();
  fireEvent.click(within(modal).getByRole('button', { name: '关闭档案' }));
  await waitFor(() => expect(screen.queryByRole('dialog', { name: '完整腕表档案' })).not.toBeInTheDocument());
  expect(card).toHaveFocus();
});
it('opens the same large modal from a video profile link', async () => {
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => resource === 'videos' ? [{ id: 1, filename: '封面.mp4' }] as never : resource === 'video' ? { id: 1, filename: '封面.mp4', profile_id: 'p2' } as never : resource === 'profile' ? { id: 'p2', model: '完整型号' } as never : [] as never);
  render(mount()); fireEvent.click(await screen.findByText('封面.mp4'));
  fireEvent.click(await screen.findByRole('button', { name: '查看关联档案' }));
  const modal = await screen.findByRole('dialog', { name: '完整腕表档案' });
  expect(await within(modal).findByRole('heading', { name: '完整型号' })).toBeInTheDocument();
  fireEvent(modal, new Event('cancel', { cancelable: true }));
  await waitFor(() => expect(screen.queryByRole('dialog', { name: '完整腕表档案' })).not.toBeInTheDocument());
});
it('requires active approved marketing access and never requests for unauthorized users', () => {
  const view = render(mount(null));
  expect(screen.getByText(/请先登录/)).toBeInTheDocument();
  view.rerender(mount({ ...user, approvalStatus: 'disabled' }));
  expect(watchLibraryApi.request).not.toHaveBeenCalled();
});
it('renders four tabs, real assets, and string-valued pagination', async () => {
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => resource === 'videos' ? Array.from({ length: 24 }, (_, id) => ({ id: id + 1, filename: `实拍${id}.mp4`, brand: '伯爵' })) as never : [] as never);
  render(mount());
  expect(screen.getAllByRole('tab')).toHaveLength(4);
  await screen.findByText('实拍0.mp4');
  fireEvent.click(screen.getByRole('button', { name: '下一页' }));
  await waitFor(() => expect(watchLibraryApi.request).toHaveBeenCalledWith(expect.objectContaining({ resource: 'videos', query: expect.objectContaining({ offset: '24', limit: '24' }) })));
});
it('reports failures and retries the real list request', async () => {
  vi.mocked(watchLibraryApi.request).mockRejectedValueOnce(new Error('网络断开'));
  render(mount());
  await screen.findByText(/网络断开/);
  fireEvent.click(screen.getByRole('button', { name: '重试加载' }));
  await screen.findByText('没有找到素材');
});
it('clears old data immediately and rejects late responses after account switch', async () => {
  let resolve!: (value: never) => void;
  vi.mocked(watchLibraryApi.request).mockImplementation(({ resource }) => resource === 'videos' ? new Promise(r => { resolve = r; }) : Promise.resolve([] as never));
  const view = render(mount());
  await waitFor(() => expect(resolve).toBeDefined());
  view.rerender(mount(null));
  await act(async () => resolve([{ id: 3, filename: '旧账户私有素材' }] as never));
  expect(screen.queryByText('旧账户私有素材')).not.toBeInTheDocument();
});
it('polls research tasks and exposes warnings and candidate differences', async () => {
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => {
    if (resource === 'match-text') return { task_id: 'task1' } as never;
    if (resource === 'task') return { id: 'task1', status: 'succeeded', result: { verdict: '待核验', warnings: ['来源不足'], results: [{ id: 4, filename: '候选素材', differences: ['表盘颜色不同'] }] } } as never;
    return [] as never;
  });
  render(mount());
  fireEvent.click(screen.getByRole('tab', { name: '型号检索' }));
  fireEvent.change(screen.getByLabelText('型号或描述'), { target: { value: 'G0A12345' } });
  fireEvent.click(screen.getByRole('button', { name: '开始检索' }));
  await screen.findByText('来源不足');
  expect(screen.getByText('表盘颜色不同')).toBeInTheDocument();
});
it('global upload window opens by event and shows only server progress and errors', async () => {
  vi.mocked(watchLibraryApi.uploads).mockResolvedValue([{ id: 'u', source: 'watch', fileName: '原片.mov', folder: '伯爵', size: 100, uploaded: 25, status: 'uploading' }]);
  vi.mocked(watchLibraryApi.uploadAction).mockRejectedValue(new Error('暂停失败'));
  render(<WatchUploadWindow user={user} />);
  act(() => window.dispatchEvent(new Event('harmony:watch-uploads-open')));
  await screen.findByText('原片.mov');
  expect(screen.getByRole('progressbar')).toHaveAttribute('value', '25');
  fireEvent.click(screen.getByRole('button', { name: '暂停' }));
  await screen.findByText('暂停失败');
});
it('distinguishes marketing and watch upload origins in the shared window', async () => {
  vi.mocked(watchLibraryApi.uploads).mockResolvedValue([
    { id: 'a', source: 'marketing', fileName: '素材.jpg', folder: '图库', size: 100, uploaded: 10, status: 'uploading' },
    { id: 'b', source: 'watch', fileName: '手表.mov', folder: '伯爵', size: 100, uploaded: 100, status: 'completed' },
  ]);
  render(<WatchUploadWindow user={user} />);
  act(() => window.dispatchEvent(new Event('harmony:watch-uploads-open')));
  await screen.findByText('素材.jpg');
  expect(screen.getByText('素材库 · 图库')).toBeInTheDocument();
  expect(screen.getByText('腕表看板 · 伯爵')).toBeInTheDocument();
});
it('rejects images larger than 8 MB without invoking image query', async () => {
  vi.mocked(open).mockResolvedValue('/large.jpg');
  vi.mocked(stat).mockResolvedValue({ size: 8 * 1024 * 1024 + 1 } as never);
  render(mount());
  fireEvent.click(screen.getByRole('tab', { name: '型号检索' }));
  fireEvent.click(screen.getByRole('button', { name: '图片查询' }));
  await screen.findByText('图片不能超过 8 MB');
  expect(watchLibraryApi.imageQuery).not.toHaveBeenCalled();
});
it('stages prepared native attachments into Redux without auto sending', async () => {
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => resource === 'videos' ? [{ id: 1, filename: '实拍.mp4' }] as never : resource === 'video' ? { id: 1, filename: '实拍.mp4' } as never : [] as never);
  vi.mocked(watchLibraryApi.prepareAsset).mockResolvedValue({ localPath: '/work/实拍.mp4', fileName: '实拍.mp4', mediaType: 'video/mp4' });
  vi.mocked(sessionsApi.create).mockResolvedValue({ id: 'new-chat' } as never);
  const dispatch = vi.spyOn(store, 'dispatch');
  render(mount());
  fireEvent.click(await screen.findByText('实拍.mp4'));
  fireEvent.click(await screen.findByRole('button', { name: '放入会话' }));
  await waitFor(() => expect(dispatch).toHaveBeenCalledWith(expect.objectContaining({ payload: expect.objectContaining({ autoSend: false, composerAttachments: [{ path: '/work/实拍.mp4', filename: '实拍.mp4', media_type: 'video/mp4' }] }) })));
});
it('surfaces native preparation failure without creating a conversation', async () => {
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => resource === 'videos' ? [{ id: 1, filename: '原片.mp4' }] as never : resource === 'video' ? { id: 1, filename: '原片.mp4' } as never : [] as never);
  vi.mocked(watchLibraryApi.prepareAsset).mockRejectedValue(new Error('附件接口不可用'));
  render(mount()); fireEvent.click(await screen.findByText('原片.mp4'));
  fireEvent.click(await screen.findByRole('button', { name: '放入会话' }));
  await screen.findByText('附件接口不可用'); expect(sessionsApi.create).not.toHaveBeenCalled();
});
it('does not poll when hidden or after upload window logout', async () => {
  const view = render(mount(user, false));
  expect(watchLibraryApi.request).not.toHaveBeenCalled();
  view.rerender(<WatchUploadWindow user={user} />);
  await waitFor(() => expect(watchLibraryApi.uploads).toHaveBeenCalledTimes(1));
  view.rerender(<WatchUploadWindow user={null} />);
  vi.useFakeTimers(); await act(async () => vi.advanceTimersByTime(10000));
  expect(watchLibraryApi.uploads).toHaveBeenCalledTimes(1);
});
it('restarts failed research instead of displaying it as success', async () => {
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => resource === 'match-text' ? { task_id: 'failed1' } as never : resource === 'task' ? { id: 'failed1', status: 'failed', error: '研究失败' } as never : [] as never);
  render(mount()); fireEvent.click(screen.getByRole('tab', { name: '型号检索' }));
  fireEvent.change(screen.getByLabelText('型号或描述'), { target: { value: 'ABC123' } }); fireEvent.click(screen.getByRole('button', { name: '开始检索' }));
  await screen.findByText('研究失败');
  fireEvent.click(screen.getByRole('button', { name: '开始检索' }));
  await waitFor(() => expect(vi.mocked(watchLibraryApi.request).mock.calls.filter(([r]) => r.resource === 'match-text')).toHaveLength(2));
});
it('shows uploaded result profile and allows inspecting its original asset', async () => {
  vi.mocked(watchLibraryApi.request).mockImplementation(async ({ resource }) => resource === 'uploads-config' ? { enabled: true, folders: ['伯爵'], extensions: ['mp4'] } as never : resource === 'uploads' ? [{ id: 'up1', filename: '上传.mp4', asset_id: 7, status: 'saved', asset: { id: 7, filename: '上传.mp4', profile: { model: 'G0A' } } }] as never : [] as never);
  render(mount()); fireEvent.click(screen.getByRole('tab', { name: '上传素材' }));
  await screen.findByText('上传.mp4');
  expect(screen.getByRole('button', { name: '查看入库素材' })).toBeInTheDocument();
});
