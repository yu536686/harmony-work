# 客户端实现任务简报

基于用户已确认的 release/verification/watch-dashboard/index.html 原型，为正式 React 应用实现新看板。只编辑 src/components/WatchLibrary/** 与 src/services/tauri/watchLibrary.ts；不要修改 App、导航、Rust 或版本文件，主代理负责集成。当前工作树 /Volumes/SSD/harmony-work-windows-1.1.3-rc1，分支 feat/watch-dashboard-1.1.4-rc1。

全部使用真实接口，不放演示数据、定时伪造进度或假成功。界面中文、Lucide 图标、现有 UI 颜色与 macOS 平台 CSS，紧凑易用，窄窗口不溢出。组件 props: active:boolean,user:MarketingUser|null,onNavigateToChat:()=>void。组件名 WatchLibrary。全局上传窗口单独导出 WatchUploadWindow，App 外层持续挂载。主动保存和加入会话使用既有 Redux 会话模式，可参考 Marketing.tsx；若原生附件接口还未准备好，显示真实错误，不伪装成功。

Rust 命令契约（主代理实现）：
- watch_library_request({request:{resource,id?,query?,body?}}) 返回上游 JSON。resource 枚举：stats,series,videos,video,profiles,profile,task,scan-status,ai-status,scan,analyze,match-text,edit,uploads-config,uploads,upload-analyze。query 是 string 值对象；body 为 JSON。video/edit id 数字字符；profile/task 为受限 ID。
- watch_library_image_query({filePath}) -> {task_id:string}。
- watch_library_media({assetId:number,kind:'cover'|'play'|'download'}) -> {url:string,expiresAt?:number,mimeType?:string}。play url 可是 HLS；客户端使用 HLS 库/原生支持，不要把 HLS 当普通 mp4；可先 HTML video 并明确错误，主代理后续完善。
- watch_library_prepare_asset({assetId:number,workspaceRoot?:string}) -> {localPath,fileName,mediaType}。
- watch_library_uploads() -> WatchUploadTask[]。
- watch_library_upload_start({filePaths:string[],folder:string}) -> WatchUploadTask[]。
- watch_library_upload_action({taskId:string,action:'pause'|'resume'|'cancel'|'retry'}) -> void。
- WatchUploadTask={id:string,fileName:string,source:'watch',folder:string,size:number,uploaded:number,status:'queued'|'uploading'|'paused'|'verifying'|'saving'|'completed'|'failed'|'cancelled',error?:string,bytesPerSecond?:number}。

接口类型从实际 231 Python/JS 源码读取：/Users/yutianqi/Documents/小亨workbuddy/watch-library-worktree/{app.py,watchlib/static/app.js,watchlib/uploads.py,watchlib/static/uploads.js,watchlib/store.py}。只读别的仓库，不能改它。

功能：四个视图（已拍摄内容、型号检索、共享档案、上传素材），分页、封面、详情、关联档案和来源、管理员编辑/scan/analyze，研究任务轮询包含警告和结果，图片查询 max8MB，上传配置、个人历史与手动分析。统一上传任务窗口通过 window 自定义事件 harmony:watch-uploads-open 打开，轮询时只在挂载且用户有权时运行，注销/换用户清空数据并阻止旧请求回填。空态、错误、重试、loading 都要做。

测试先失败再实现，Vitest/Testing Library。报告写 docs/superpowers/plans/watch-dashboard-client-report.md，列实际命令与测试结果及未覆盖功能；不要自行 commit。返回简短总结与更改文件清单。
