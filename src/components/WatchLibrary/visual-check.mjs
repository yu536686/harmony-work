// Isolated visual test fixtures only; the production component always invokes native APIs.
import { createServer } from 'vite';
import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const server = await createServer({ root, configFile: false, cacheDir: 'src/components/WatchLibrary/.visual-cache', server: { host: '127.0.0.1', port: 0, hmr: false, watch: null }, optimizeDeps: { include: ['react', 'react-dom/client', 'react-redux', '@reduxjs/toolkit', 'lucide-react', 'react/jsx-dev-runtime'] }, esbuild: { jsx: 'automatic' } });
server.middlewares.use('/__watch_verify', (_req, res) => {
  res.setHeader('Content-Type', 'text/html; charset=utf-8');
  res.end(`<!doctype html><html data-platform="macos" data-color-mode="light"><head><style>html,body,#root{margin:0;height:100%;font-family:system-ui}*{box-sizing:border-box}</style></head><body><div id="root"></div><script type="module">
import React from '/src/components/WatchLibrary/.visual-cache/deps/react.js';
import ReactDOM from '/src/components/WatchLibrary/.visual-cache/deps/react-dom_client.js';
const {createRoot} = ReactDOM;
import {Provider} from '/src/components/WatchLibrary/.visual-cache/deps/react-redux.js';
import {configureStore} from '/src/components/WatchLibrary/.visual-cache/deps/@reduxjs_toolkit.js';
import {WatchLibrary, WatchUploadWindow} from '/src/components/WatchLibrary/index.tsx';
import '/src/theme.css'; import '/src/macos.css';
const videos = Array.from({length:6}, (_,i)=>({id:i+1,filename:'伯爵_实拍素材_长文件名验证_'+i+'.mp4',brand:'伯爵',series_cn:'Limelight',size:123456789,duration:45,analyzed_at:'2026-10-28'}));
window.__TAURI_INTERNALS__ = {invoke: async (command,args) => {
 if(command==='watch_library_media') return {url:location.origin+'/release/verification/watch-dashboard/assets/watch-c.jpg'};
 if(command==='watch_library_uploads') return [{id:'one',fileName:'伯爵_原始拍摄素材.mov',source:'watch',folder:'伯爵',size:100000000,uploaded:25000000,status:'uploading',bytesPerSecond:1000000}];
 if(command==='watch_library_request') {
  const r=args.request;
  if(r.resource==='stats') return {count:81,ai:76,total_bytes:12345678900,brands:[{brand:'伯爵',n:6}]};
  if(r.resource==='series') return [{series_cn:'Limelight',n:6}];
  if(r.resource==='videos') return videos;
  if(r.resource==='video') return {...videos[0],profile:{id:'p1',model:'G0A12345',brand:'伯爵',series:'Limelight',sources:[{url:'https://example.com',title:'来源核验'}]}};
  if(r.resource==='profiles') return [{id:'p1',model:'G0A12345',brand:'伯爵',series:'Limelight',material:'18K玫瑰金',dial:'银色表盘',tags:['珠宝腕表','自动上链'],sources:[{url:'https://example.com'}]}];
  if(r.resource==='profile') return {id:'p1',model:'G0A12345',brand:'伯爵',series:'Limelight',material:'18K玫瑰金',dial:'银色表盘',bracelet:'皮革表带',description:'腕表共享档案，包含规格、来源以及需进一步核验的信息。',tags:['珠宝腕表','自动上链'],sources:Array.from({length:18},(_,i)=>({title:'资料来源 '+(i+1),url:'https://example.com/'+i,snippet:'用于验证长档案在弹层内部滚动，背景页面不移动。'}))};
  if(r.resource==='uploads-config') return {enabled:true,folders:['伯爵'],extensions:['mp4','mov'],max_file_bytes:1000000000};
  return [];
 }
 throw new Error('测试未提供该原生命令');
}};
const user={id:'visual',name:'测试',email:'visual@test.cn',userRole:'user',accountType:'marketing',approvalStatus:'active'};
const store=configureStore({reducer:()=>({sessions:{activeSessionId:null},settings:{settings:{workspace_root:'/work'}}})});
createRoot(document.getElementById('root')).render(React.createElement(Provider,{store},React.createElement(React.Fragment,null,React.createElement(WatchLibrary,{active:true,user,onNavigateToChat:()=>{}}),React.createElement(WatchUploadWindow,{user}))));
</script></body></html>`);
});
// Mount the fixture before Vite's SPA fallback so it does not load the real app.
server.middlewares.stack.unshift(server.middlewares.stack.pop());
let browser;
try {
  await server.listen();
  const address = server.httpServer.address();
  browser = await chromium.launch({ headless: true, channel: 'chrome' });
  const page = await browser.newPage();
  const errors = []; page.on('pageerror', e => { errors.push(e.message); console.error(e.message); });
  page.on('response', r => { if (r.status() >= 400) console.error('HTTP', r.status(), r.url()); });
  page.on('console', e => { if (e.type() === 'error') console.error(e.text()); });
  for (const width of [1440, 760, 390]) {
    await page.setViewportSize({ width, height: 950 });
    await page.goto(`http://127.0.0.1:${address.port}/__watch_verify`);
    try { await page.getByText('伯爵_实拍素材_长文件名验证_0.mp4', {exact:true}).click({timeout:5000}); }
    catch (error) { console.error('fixture text:', await page.locator('body').innerText()); throw error; }
    await page.getByRole('button',{name:'放入会话'}).waitFor();
    await page.getByRole('button',{name:'查看完整档案',exact:true}).click();
    const modal = page.getByRole('dialog',{name:'完整腕表档案'});
    await modal.waitFor();
    await modal.getByRole('heading',{name:'G0A12345'}).waitFor();
    for (const mode of ['light', 'dark']) {
      await page.evaluate(mode => { document.documentElement.dataset.platform='macos'; document.documentElement.dataset.colorMode=mode; }, mode);
      await page.waitForTimeout(240);
      const foreground = await modal.evaluate(el => getComputedStyle(el).color);
      const brightness = foreground.match(/[\d.]+/g).slice(0,3).reduce((sum,n)=>sum+Number(n),0)/3;
      assert(mode === 'dark' ? brightness > 160 : brightness < 120, 'theme not applied: '+foreground);
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'page overflow');
      const box = await modal.boundingBox();
      assert(box && box.x >= 0 && box.y >= 0 && box.x+box.width <= width+1 && box.y+box.height <= 951, 'modal not framed');
      assert(await modal.locator('.watch-modal-body').evaluate(el=>el.scrollHeight>el.clientHeight), 'long profile should scroll inside modal');
      const path = fileURLToPath(new URL(`./visual-${width}-${mode}.png`, import.meta.url));
      await page.screenshot({ path, fullPage:true });
    }
    await page.keyboard.press('Escape');
    await modal.waitFor({state:'hidden'});
    assert(await page.getByRole('button',{name:'查看完整档案',exact:true}).evaluate(el=>el===document.activeElement), 'focus not restored');
    await page.getByRole('tab',{name:'共享档案'}).click();
    await page.getByRole('button',{name:/G0A12345/}).click();
    await modal.waitFor();
    await page.mouse.click(2,2);
    await modal.waitFor({state:'hidden'});
    await page.getByRole('tab',{name:'上传素材'}).click();
    await page.getByText('还没有上传记录').waitFor();
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'upload page overflow');
    await page.getByRole('button',{name:'上传任务',exact:true}).click();
    await page.getByRole('dialog',{name:'上传任务'}).waitFor();
    await page.getByRole('progressbar').waitFor();
    await page.getByRole('button',{name:'关闭上传任务'}).click();
    console.log('Verified rc2 modal, scroll, Esc/backdrop, focus, light/dark and uploads:', width);
  }
  assert.deepEqual(errors, []);
  if (process.env.WATCH_VISUAL_PREVIEW === '1') {
    await browser.close(); browser = undefined;
    console.log(`RC2 component preview (fixture data): http://127.0.0.1:${address.port}/__watch_verify`);
    await new Promise(() => {});
  }
} finally { await browser?.close(); await server.close(); }
