import { invoke } from '@tauri-apps/api/core';

export type WatchResource = 'stats' | 'series' | 'videos' | 'video' | 'profiles' | 'profile' | 'task' | 'scan-status' | 'ai-status' | 'scan' | 'analyze' | 'match-text' | 'edit' | 'uploads-config' | 'uploads' | 'upload-analyze';
export interface WatchRequest { resource: WatchResource; id?: string; query?: Record<string, string>; body?: unknown }
export interface WatchProfile {
  id?: string; brand?: string; brand_cn?: string; series?: string; series_cn?: string; model?: string;
  material?: string; dial?: string; bracelet?: string; bezel?: string; case_size_mm?: number;
  status?: string; description?: string; confidence?: number; tags?: string[]; warnings?: string[];
  uncertainties?: string[]; updated_at?: string;
  sources?: Array<string | { url?: string; page_url?: string; source_url?: string; title?: string; snippet?: string; type?: string; reviewed_by?: string; reviewed_at?: string }>;
  field_evidence?: Record<string, unknown>;
}
export interface WatchVideo { id: number; filename: string; brand?: string; series?: string; series_cn?: string; ref?: string; model_code?: string; size?: number; duration?: number; width?: number; height?: number; analyzed_at?: string; review_status?: string; profile_id?: string; profile?: WatchProfile; ai?: WatchProfile }
export interface WatchCandidate extends Partial<WatchVideo> { video?: WatchVideo | string; video_id?: number; verdict?: string; match_type?: string; score?: number; reasons?: string[]; differences?: string[] }
export interface WatchResult { verdict?: string; exact?: boolean; research_complete?: boolean; warnings?: string[]; errors?: string[]; profile?: WatchProfile; parsed?: WatchProfile; results?: WatchCandidate[]; best?: WatchCandidate; trace?: Array<string | { stage?: string; status?: string; message?: string; url?: string }> }
export interface WatchTask { id: string; status: string; stage?: string; total?: number; done?: number; error?: string; running?: boolean; result?: WatchResult }
export interface WatchStats { count?: number; ai?: number; embed?: number; total_bytes?: number; brands?: Array<{ brand: string; n: number }> }
export interface WatchUploadConfig { enabled: boolean; folders: string[]; extensions: string[]; max_file_bytes: number; chunk_bytes: number }
export interface WatchUploadRecord { id: string; filename: string; folder: string; size: number; offset: number; status: string; asset_id?: number; destination?: string; created_at: string; index_task?: WatchTask; analysis_task?: WatchTask; asset?: Partial<WatchVideo> }
export interface WatchUploadTask { id: string; fileName: string; source: 'watch' | 'marketing'; folder: string; size: number; uploaded: number; status: 'queued' | 'uploading' | 'paused' | 'verifying' | 'saving' | 'completed' | 'failed' | 'cancelled' | 'cancelling'; error?: string; bytesPerSecond?: number }
export interface WatchMedia { url: string; expiresAt?: number; mimeType?: string }
export interface WatchPreparedAsset { localPath: string; fileName: string; mediaType: string }

export const watchLibraryApi = {
  request: <T = unknown>(request: WatchRequest): Promise<T> => invoke('watch_library_request', { request }),
  imageQuery: (filePath: string): Promise<{ task_id: string }> => invoke('watch_library_image_query', { filePath }),
  media: (assetId: number, kind: 'cover' | 'play' | 'download'): Promise<WatchMedia> => invoke('watch_library_media', { assetId, kind }),
  prepareAsset: (assetId: number, workspaceRoot?: string): Promise<WatchPreparedAsset> => invoke('watch_library_prepare_asset', { assetId, workspaceRoot }),
  uploads: (): Promise<WatchUploadTask[]> => invoke('watch_library_uploads'),
  uploadStart: (filePaths: string[], folder: string): Promise<WatchUploadTask[]> => invoke('watch_library_upload_start', { filePaths, folder }),
  marketingUploadStart: (filePaths: string[], folder: string): Promise<WatchUploadTask[]> => invoke('marketing_resumable_upload_start', { filePaths, folder }),
  uploadAction: (taskId: string, action: 'pause' | 'resume' | 'cancel' | 'retry'): Promise<void> => invoke('watch_library_upload_action', { taskId, action }),
};
