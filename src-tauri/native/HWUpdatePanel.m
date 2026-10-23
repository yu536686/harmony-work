#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import <objc/runtime.h>

static const void *HWUpdatePanelKey = &HWUpdatePanelKey;

@interface HWUpdateController : NSObject <NSWindowDelegate>
@property(nonatomic, strong) NSPanel *panel;
@property(nonatomic, weak) WKWebView *webView;
@property(nonatomic, copy) NSDictionary *snapshot;
@property(nonatomic, strong) NSTextField *heading;
@property(nonatomic, strong) NSTextField *version;
@property(nonatomic, strong) NSTextField *status;
@property(nonatomic, strong) NSTextView *notes;
@property(nonatomic, strong) NSProgressIndicator *progress;
@property(nonatomic, strong) NSButton *primary;
@property(nonatomic, strong) NSButton *cancel;
@property(nonatomic, strong) NSSwitch *preview;
@property(nonatomic) BOOL suppressClose;
- (instancetype)initWithWindow:(NSWindow *)window webView:(WKWebView *)webView;
- (void)update:(NSDictionary *)state;
@end

static NSTextField *HWUpdateLabel(NSString *text, NSRect frame, CGFloat size) {
    NSTextField *label = [NSTextField wrappingLabelWithString:text];
    label.frame = frame;
    label.font = [NSFont systemFontOfSize:size];
    return label;
}

@implementation HWUpdateController
- (instancetype)initWithWindow:(NSWindow *)window webView:(WKWebView *)webView {
    if (!(self = [super init])) return nil;
    self.webView = webView;
    self.panel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 480, 350)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
        backing:NSBackingStoreBuffered defer:NO];
    self.panel.title = @"Harmony Work 更新";
    self.panel.delegate = self;
    self.panel.appearance = window.appearance;
    self.panel.releasedWhenClosed = NO;
    NSVisualEffectView *view = [[NSVisualEffectView alloc] initWithFrame:self.panel.contentView.bounds];
    view.material = NSVisualEffectMaterialWindowBackground;
    view.blendingMode = NSVisualEffectBlendingModeBehindWindow;
    view.state = NSVisualEffectStateActive;
    self.panel.contentView = view;
    NSImageView *icon = [[NSImageView alloc] initWithFrame:NSMakeRect(24, 280, 44, 44)];
    icon.image = NSApp.applicationIconImage;
    [view addSubview:icon];
    self.heading = HWUpdateLabel(@"检查 Harmony Work 更新", NSMakeRect(82, 298, 372, 26), 18);
    self.heading.font = [NSFont systemFontOfSize:18 weight:NSFontWeightSemibold];
    [view addSubview:self.heading];
    self.version = HWUpdateLabel(@"", NSMakeRect(82, 275, 372, 22), 12);
    self.version.textColor = NSColor.secondaryLabelColor;
    [view addSubview:self.version];
    NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(24, 152, 432, 110)];
    scroll.hasVerticalScroller = YES;
    scroll.autohidesScrollers = YES;
    scroll.drawsBackground = NO;
    self.notes = [[NSTextView alloc] initWithFrame:scroll.bounds];
    self.notes.font = [NSFont systemFontOfSize:13];
    self.notes.editable = NO;
    self.notes.drawsBackground = NO;
    self.notes.textColor = NSColor.labelColor;
    self.notes.autoresizingMask = NSViewWidthSizable;
    self.notes.textContainer.widthTracksTextView = YES;
    scroll.documentView = self.notes;
    [view addSubview:scroll];
    self.status = HWUpdateLabel(@"", NSMakeRect(24, 103, 432, 42), 12);
    [view addSubview:self.status];
    self.progress = [[NSProgressIndicator alloc] initWithFrame:NSMakeRect(24, 89, 432, 5)];
    self.progress.style = NSProgressIndicatorStyleBar;
    self.progress.maxValue = 100;
    [view addSubview:self.progress];
    self.preview = [[NSSwitch alloc] initWithFrame:NSMakeRect(24, 30, 38, 24)];
    self.preview.target = self;
    self.preview.action = @selector(togglePreview:);
    [self.preview setAccessibilityLabel:@"接收测试版本更新"];
    [view addSubview:self.preview];
    NSTextField *previewLabel = HWUpdateLabel(@"接收测试版", NSMakeRect(70, 32, 105, 21), 12);
    [view addSubview:previewLabel];
    self.cancel = [NSButton buttonWithTitle:@"关闭" target:self action:@selector(cancelAction:)];
    self.cancel.frame = NSMakeRect(238, 25, 100, 32);
    [view addSubview:self.cancel];
    self.primary = [NSButton buttonWithTitle:@"检查更新" target:self action:@selector(primaryAction:)];
    self.primary.frame = NSMakeRect(344, 25, 112, 32);
    self.primary.keyEquivalent = @"\r";
    self.primary.bezelColor = NSColor.controlAccentColor;
    [view addSubview:self.primary];
    if (window) [self.panel setFrameOrigin:NSMakePoint(NSMidX(window.frame) - 240, NSMidY(window.frame) - 175)];
    else [self.panel center];
    return self;
}
- (void)dispatch:(NSString *)action {
    if (!self.webView) return;
    NSData *data = [NSJSONSerialization dataWithJSONObject:@{@"action": action} options:0 error:nil];
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    [self.webView evaluateJavaScript:[NSString stringWithFormat:@"window.dispatchEvent(new CustomEvent('harmony:native-update-action',{detail:%@}))", json] completionHandler:nil];
}
- (void)primaryAction:(id)sender { [self dispatch:self.snapshot[@"available"] != NSNull.null && self.snapshot[@"available"] ? @"install" : @"check"]; }
- (void)togglePreview:(id)sender { [self dispatch:self.preview.state == NSControlStateValueOn ? @"preview" : @"stable"]; }
- (void)cancelAction:(id)sender {
    NSString *phase = self.snapshot[@"phase"];
    if ([phase isEqual:@"downloading"] || [phase isEqual:@"waiting"]) [self dispatch:@"cancel"];
    else [self.panel close];
}
- (BOOL)windowShouldClose:(NSWindow *)sender { return ![self.snapshot[@"phase"] isEqual:@"installing"]; }
- (void)windowWillClose:(NSNotification *)notification { if (!self.suppressClose) [self dispatch:@"close"]; }
- (void)update:(NSDictionary *)state {
    self.snapshot = state;
    NSString *phase = state[@"phase"] ?: @"idle";
    BOOL busy = [@[@"checking", @"downloading", @"verifying", @"waiting", @"installing"] containsObject:phase];
    NSDictionary *available = [state[@"available"] isKindOfClass:NSDictionary.class] ? state[@"available"] : nil;
    NSString *current = state[@"currentVersion"] ?: @"";
    self.heading.stringValue = available ? @"Harmony Work 可用更新" : [phase isEqual:@"current"] ? @"已是最新版本" : @"检查 Harmony Work 更新";
    self.version.stringValue = available ? [NSString stringWithFormat:@"v%@ → v%@", current, available[@"version"] ?: @""] : [NSString stringWithFormat:@"当前版本 v%@", current];
    self.notes.string = available[@"notes"] ?: @"更新将保留会话、设置、记忆和工作区。";
    NSDictionary *labels = @{@"checking": @"正在检查可用版本…", @"downloading": @"正在下载更新…", @"verifying": @"正在验证更新包…", @"waiting": @"等待当前任务结束；可以取消等待。", @"installing": @"正在准备安装，完成后将自动重新打开。", @"current": @"当前已安装最新版本。", @"idle": @"点击检查更新以获取可用版本。"};
    NSString *error = [state[@"error"] isKindOfClass:NSString.class] ? state[@"error"] : nil;
    self.status.stringValue = error ? @"暂时无法完成更新操作，请稍后重试。详情见上方。" : labels[phase] ?: @"可以安装此更新。";
    if (error) self.notes.string = [NSString stringWithFormat:@"%@\n\n错误详情：\n%@", self.notes.string, error];
    self.status.textColor = error ? NSColor.systemRedColor : NSColor.secondaryLabelColor;
    NSNumber *total = [state[@"total"] isKindOfClass:NSNumber.class] ? state[@"total"] : nil;
    self.progress.hidden = !busy;
    self.progress.indeterminate = total.doubleValue <= 0 || ![phase isEqual:@"downloading"];
    if (self.progress.indeterminate) [self.progress startAnimation:nil];
    else {
        [self.progress stopAnimation:nil];
        double downloaded = [state[@"downloaded"] doubleValue];
        self.progress.doubleValue = MIN(100, downloaded / total.doubleValue * 100);
        self.status.stringValue = [NSString stringWithFormat:@"正在下载 %.1f / %.1f MB · %.0f%%", downloaded / 1000000, total.doubleValue / 1000000, self.progress.doubleValue];
    }
    self.primary.title = available ? @"安装并重启" : @"检查更新";
    self.primary.enabled = !busy;
    self.preview.enabled = !busy;
    self.preview.state = [state[@"channel"] isEqual:@"preview"] ? NSControlStateValueOn : NSControlStateValueOff;
    BOOL cancellable = [phase isEqual:@"downloading"] || [phase isEqual:@"waiting"];
    self.cancel.title = cancellable ? @"取消" : @"关闭";
    self.cancel.enabled = ![phase isEqual:@"installing"];
    [self.panel standardWindowButton:NSWindowCloseButton].enabled = self.cancel.enabled;
}
@end

bool hw_show_update(void *windowPointer, void *webViewPointer, const char *jsonPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    WKWebView *webView = (__bridge WKWebView *)webViewPointer;
    if (!window || !webView || !jsonPointer) return false;
    NSData *data = [[NSString stringWithUTF8String:jsonPointer] dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *state = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![state isKindOfClass:NSDictionary.class]) return false;
    HWUpdateController *controller = objc_getAssociatedObject(window, HWUpdatePanelKey);
    if (!controller) {
        controller = [[HWUpdateController alloc] initWithWindow:window webView:webView];
        objc_setAssociatedObject(window, HWUpdatePanelKey, controller, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
    [controller update:state];
    if (!controller.panel.visible) [controller.panel makeKeyAndOrderFront:nil];
    return true;
}

void hw_close_update(void *windowPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    HWUpdateController *controller = objc_getAssociatedObject(window, HWUpdatePanelKey);
    controller.suppressClose = YES;
    [controller.panel close];
    objc_setAssociatedObject(window, HWUpdatePanelKey, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}

void hw_update_helper_progress(const char *phase, const char *detail, bool failed, bool finished) {
    static HWUpdateController *controller;
    if (!controller) {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        controller = [[HWUpdateController alloc] initWithWindow:nil webView:nil];
        [controller.panel makeKeyAndOrderFront:nil];
        [NSApp activateIgnoringOtherApps:YES];
        [NSApp finishLaunching];
    }
    [controller update:@{@"phase": @"installing", @"currentVersion": @"", @"channel": @"stable"}];
    controller.version.stringValue = @"Harmony Work";
    NSString *stage = phase ? [NSString stringWithUTF8String:phase] : @"verifying";
    NSDictionary *labels = @{@"verifying": @"正在验证更新", @"waiting": @"等待客户端退出", @"installing": @"正在安装更新", @"restarting": @"正在重新打开", @"rollback": @"正在恢复上一版本", @"cleanup": @"正在完成更新", @"finished": @"更新完成", @"error": @"更新未完成"};
    controller.heading.stringValue = labels[stage] ?: @"正在更新 Harmony Work";
    controller.status.stringValue = failed && detail ? [NSString stringWithUTF8String:detail] : @"请稍候，完成后会自动打开客户端。";
    controller.notes.string = @"更新完成后将自动打开 Harmony Work。";
    controller.primary.hidden = YES;
    controller.preview.hidden = YES;
    controller.cancel.hidden = YES;
    for (NSView *view in controller.panel.contentView.subviews) {
        if ([view isKindOfClass:NSTextField.class] && [((NSTextField *)view).stringValue isEqual:@"接收测试版"]) view.hidden = YES;
    }
    NSDate *until = [NSDate dateWithTimeIntervalSinceNow:0.02];
    while (YES) {
        NSEvent *event = [NSApp nextEventMatchingMask:NSEventMaskAny untilDate:until inMode:NSDefaultRunLoopMode dequeue:YES];
        if (!event) break;
        [NSApp sendEvent:event];
    }
    [NSApp updateWindows];
    if (failed) {
        NSAlert *alert = [NSAlert new];
        alert.messageText = @"Harmony Work 更新未完成";
        alert.informativeText = controller.status.stringValue;
        alert.alertStyle = NSAlertStyleWarning;
        [alert addButtonWithTitle:@"好"];
        [alert runModal];
    }
    if (finished || failed) { [controller.panel orderOut:nil]; controller = nil; }
}
