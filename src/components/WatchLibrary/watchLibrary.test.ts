import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { watchLibraryApi } from '../../services/tauri/watchLibrary';

beforeEach(() => { vi.mocked(invoke).mockReset(); });
it('preserves upstream array and string pagination query contract', async () => {
  vi.mocked(invoke).mockResolvedValue([{ id: 12, filename: '素材.mp4' }]);
  expect(await watchLibraryApi.request({ resource: 'videos', query: { offset: '24', limit: '24' } })).toEqual([{ id: 12, filename: '素材.mp4' }]);
  expect(invoke).toHaveBeenCalledWith('watch_library_request', { request: { resource: 'videos', query: { offset: '24', limit: '24' } } });
});
it('uses exact native media, image, prepare and upload envelopes', async () => {
  await watchLibraryApi.media(12, 'play');
  await watchLibraryApi.imageQuery('/image.jpg');
  await watchLibraryApi.prepareAsset(12, '/workspace');
  await watchLibraryApi.uploadStart(['/a.mov'], '伯爵');
  await watchLibraryApi.uploadAction('abc', 'pause');
  await watchLibraryApi.uploads();
  expect(vi.mocked(invoke).mock.calls).toEqual([
    ['watch_library_media', { assetId: 12, kind: 'play' }],
    ['watch_library_image_query', { filePath: '/image.jpg' }],
    ['watch_library_prepare_asset', { assetId: 12, workspaceRoot: '/workspace' }],
    ['watch_library_upload_start', { filePaths: ['/a.mov'], folder: '伯爵' }],
    ['watch_library_upload_action', { taskId: 'abc', action: 'pause' }],
    ['watch_library_uploads'],
  ]);
});
it('does not swallow native errors', async () => {
  vi.mocked(invoke).mockRejectedValue(new Error('原生附件接口尚未准备好'));
  await expect(watchLibraryApi.prepareAsset(1)).rejects.toThrow('原生附件接口尚未准备好');
});
it('submits marketing assets to the shared native queue', async () => {
  await watchLibraryApi.marketingUploadStart(['/asset.jpg'], '图库');
  expect(invoke).toHaveBeenCalledWith('marketing_resumable_upload_start', { filePaths: ['/asset.jpg'], folder: '图库' });
});
