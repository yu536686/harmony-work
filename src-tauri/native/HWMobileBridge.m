#import <AppKit/AppKit.h>
#import <CoreImage/CoreImage.h>
#import <WebKit/WebKit.h>
#import <objc/runtime.h>
#include <math.h>

static const void *HWMobileBridgeKey = &HWMobileBridgeKey;

@interface HWMobilePhoneRows : NSStackView
@end
@implementation HWMobilePhoneRows
- (BOOL)isFlipped { return YES; }
@end

static NSString *HWMobileString(id value) {
    return [value isKindOfClass:NSString.class] ? value : @"";
}

static NSDictionary *HWMobileOptions(const char *json) {
    if (!json) return nil;
    NSString *string = [NSString stringWithUTF8String:json];
    if (!string) return nil;
    id result = [NSJSONSerialization JSONObjectWithData:[string dataUsingEncoding:NSUTF8StringEncoding]
        options:0 error:nil];
    if (![result isKindOfClass:NSDictionary.class] ||
        ![result[@"state"] isKindOfClass:NSDictionary.class] ||
        !HWMobileString(result[@"panelId"]).length) return nil;
    return result;
}

static NSTextField *HWMobileLabel(NSString *text, CGFloat size, NSFontWeight weight) {
    NSTextField *label = [NSTextField wrappingLabelWithString:text];
    label.font = [NSFont systemFontOfSize:size weight:weight];
    label.selectable = YES;
    [label setContentCompressionResistancePriority:NSLayoutPriorityDefaultLow
        forOrientation:NSLayoutConstraintOrientationHorizontal];
    return label;
}

static NSImage *HWMobileQRCode(NSString *uri) {
    if (!uri.length) return nil;
    CIFilter *filter = [CIFilter filterWithName:@"CIQRCodeGenerator"];
    [filter setValue:[uri dataUsingEncoding:NSUTF8StringEncoding] forKey:@"inputMessage"];
    [filter setValue:@"M" forKey:@"inputCorrectionLevel"];
    CIImage *output = filter.outputImage;
    if (!output || CGRectIsEmpty(output.extent)) return nil;
    CIContext *context = [CIContext contextWithOptions:nil];
    CGImageRef pixels = [context createCGImage:output fromRect:output.extent];
    if (!pixels) return nil;
    NSImage *source = [[NSImage alloc] initWithCGImage:pixels size:output.extent.size];
    CGImageRelease(pixels);
    // Integer scaling and a white quiet zone keep the pairing code scannable.
    CGFloat scale = floor(220.0 / (output.extent.size.width + 8.0));
    if (scale < 1) return nil;
    CGFloat side = output.extent.size.width * scale;
    NSImage *image = [[NSImage alloc] initWithSize:NSMakeSize(220, 220)];
    [image lockFocus];
    [NSColor.whiteColor setFill];
    NSRectFill(NSMakeRect(0, 0, 220, 220));
    NSGraphicsContext.currentContext.imageInterpolation = NSImageInterpolationNone;
    [source drawInRect:NSMakeRect((220 - side) / 2, (220 - side) / 2, side, side)
        fromRect:NSZeroRect operation:NSCompositingOperationCopy fraction:1];
    [image unlockFocus];
    return image;
}

@interface HWMobileBridgeController : NSObject <NSWindowDelegate>
@property (nonatomic, weak) NSWindow *parent;
@property (nonatomic, weak) WKWebView *webView;
@property (nonatomic, strong) NSPanel *panel;
@property (nonatomic, copy) NSString *panelId;
@property (nonatomic, copy) NSDictionary *state;
@property (nonatomic, copy) NSString *pairingUri;
@property (nonatomic, strong) NSImageView *qrView;
@property (nonatomic, strong) NSTextField *placeholder;
@property (nonatomic, strong) NSTextField *statusLabel;
@property (nonatomic, strong) NSTextField *errorLabel;
@property (nonatomic, strong) NSTextField *phonesLabel;
@property (nonatomic, strong) NSStackView *phoneRows;
@property (nonatomic, strong) NSButton *generateButton;
@property (nonatomic, strong) NSButton *refreshButton;
@property (nonatomic, strong) NSButton *closeButton;
@property (nonatomic, strong) NSProgressIndicator *progress;
@property (nonatomic, strong) NSMutableArray<NSButton *> *unlinkButtons;
@property (nonatomic) BOOL actionPending;
@property (nonatomic) BOOL confirming;
@property (nonatomic) BOOL dismissed;
- (void)build;
- (void)applyState:(NSDictionary *)state;
- (void)updateEnabled;
- (void)dismiss;
@end

@implementation HWMobileBridgeController

- (BOOL)isBusy {
    return [self.state[@"busy"] boolValue] || self.actionPending || self.confirming || self.dismissed;
}

- (NSButton *)button:(NSString *)title action:(SEL)action {
    NSButton *button = [NSButton buttonWithTitle:title target:self action:action];
    button.bezelStyle = NSBezelStyleRounded;
    return button;
}

- (void)build {
    self.panel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 560, 724)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
        backing:NSBackingStoreBuffered defer:NO];
    self.panel.title = @"连接手机";
    self.panel.releasedWhenClosed = NO;
    self.panel.delegate = self;
    self.panel.contentMinSize = NSMakeSize(560, 724);
    self.panel.contentMaxSize = NSMakeSize(560, 724);
    NSView *content = self.panel.contentView;
    NSStackView *stack = [NSStackView new];
    stack.orientation = NSUserInterfaceLayoutOrientationVertical;
    stack.alignment = NSLayoutAttributeLeading;
    stack.spacing = 8;
    stack.translatesAutoresizingMaskIntoConstraints = NO;
    [content addSubview:stack];
    [NSLayoutConstraint activateConstraints:@[
        [stack.leadingAnchor constraintEqualToAnchor:content.leadingAnchor constant:24],
        [stack.trailingAnchor constraintEqualToAnchor:content.trailingAnchor constant:-24],
        [stack.topAnchor constraintEqualToAnchor:content.topAnchor constant:20],
        [stack.bottomAnchor constraintEqualToAnchor:content.bottomAnchor constant:-20],
        [stack.widthAnchor constraintEqualToConstant:512],
    ]];
    NSTextField *title = HWMobileLabel(@"连接手机与这台 Mac", 20, NSFontWeightSemibold);
    [stack addArrangedSubview:title];
    [title.heightAnchor constraintEqualToConstant:24].active = YES;
    NSTextField *intro = HWMobileLabel(@"用同一 Harmony Work 账户登录手机，在「我的设备」扫描配对二维码。共享会话和远端任务由这台 Mac 同步与执行。", 13, NSFontWeightRegular);
    intro.textColor = NSColor.secondaryLabelColor;
    [stack addArrangedSubview:intro];
    [intro.widthAnchor constraintEqualToAnchor:stack.widthAnchor].active = YES;
    [intro.heightAnchor constraintEqualToConstant:40].active = YES;

    NSView *codeArea = [NSView new];
    [stack addArrangedSubview:codeArea];
    [codeArea.widthAnchor constraintEqualToAnchor:stack.widthAnchor].active = YES;
    [codeArea.heightAnchor constraintEqualToConstant:220].active = YES;
    self.qrView = [NSImageView new];
    self.qrView.translatesAutoresizingMaskIntoConstraints = NO;
    self.qrView.imageScaling = NSImageScaleNone;
    self.qrView.accessibilityLabel = @"配对二维码";
    [codeArea addSubview:self.qrView];
    self.placeholder = HWMobileLabel(@"", 13, NSFontWeightRegular);
    self.placeholder.alignment = NSTextAlignmentCenter;
    self.placeholder.textColor = NSColor.secondaryLabelColor;
    self.placeholder.translatesAutoresizingMaskIntoConstraints = NO;
    [codeArea addSubview:self.placeholder];
    [NSLayoutConstraint activateConstraints:@[
        [self.qrView.widthAnchor constraintEqualToConstant:220],
        [self.qrView.heightAnchor constraintEqualToConstant:220],
        [self.qrView.centerXAnchor constraintEqualToAnchor:codeArea.centerXAnchor],
        [self.qrView.centerYAnchor constraintEqualToAnchor:codeArea.centerYAnchor],
        [self.placeholder.leadingAnchor constraintEqualToAnchor:codeArea.leadingAnchor],
        [self.placeholder.trailingAnchor constraintEqualToAnchor:codeArea.trailingAnchor],
        [self.placeholder.centerYAnchor constraintEqualToAnchor:codeArea.centerYAnchor],
    ]];
    self.statusLabel = HWMobileLabel(@"", 12, NSFontWeightRegular);
    self.statusLabel.textColor = NSColor.secondaryLabelColor;
    [stack addArrangedSubview:self.statusLabel];
    [self.statusLabel.widthAnchor constraintEqualToAnchor:stack.widthAnchor].active = YES;
    [self.statusLabel.heightAnchor constraintEqualToConstant:28].active = YES;
    self.errorLabel = HWMobileLabel(@"", 12, NSFontWeightRegular);
    self.errorLabel.textColor = NSColor.systemRedColor;
    [stack addArrangedSubview:self.errorLabel];
    [self.errorLabel.widthAnchor constraintEqualToAnchor:stack.widthAnchor].active = YES;
    [self.errorLabel.heightAnchor constraintEqualToConstant:28].active = YES;

    self.generateButton = [self button:@"生成配对二维码" action:@selector(generate:)];
    self.generateButton.keyEquivalent = @"\r";
    self.refreshButton = [self button:@"刷新设备" action:@selector(refresh:)];
    self.progress = [NSProgressIndicator new];
    self.progress.style = NSProgressIndicatorStyleSpinning;
    self.progress.controlSize = NSControlSizeSmall;
    self.progress.displayedWhenStopped = NO;
    NSStackView *actions = [NSStackView stackViewWithViews:@[self.generateButton, self.refreshButton, self.progress]];
    actions.spacing = 8;
    [stack addArrangedSubview:actions];
    [actions.heightAnchor constraintEqualToConstant:32].active = YES;
    self.phonesLabel = HWMobileLabel(@"已绑定手机", 13, NSFontWeightSemibold);
    [stack addArrangedSubview:self.phonesLabel];
    [self.phonesLabel.heightAnchor constraintEqualToConstant:18].active = YES;

    NSScrollView *scroll = [NSScrollView new];
    scroll.hasVerticalScroller = YES;
    scroll.autohidesScrollers = YES;
    scroll.drawsBackground = NO;
    self.phoneRows = [HWMobilePhoneRows new];
    self.phoneRows.orientation = NSUserInterfaceLayoutOrientationVertical;
    self.phoneRows.alignment = NSLayoutAttributeLeading;
    self.phoneRows.spacing = 8;
    self.phoneRows.translatesAutoresizingMaskIntoConstraints = NO;
    scroll.documentView = self.phoneRows;
    [stack addArrangedSubview:scroll];
    [NSLayoutConstraint activateConstraints:@[
        [scroll.widthAnchor constraintEqualToAnchor:stack.widthAnchor],
        [scroll.heightAnchor constraintEqualToConstant:80],
        [self.phoneRows.leadingAnchor constraintEqualToAnchor:scroll.contentView.leadingAnchor],
        [self.phoneRows.trailingAnchor constraintEqualToAnchor:scroll.contentView.trailingAnchor],
        [self.phoneRows.topAnchor constraintEqualToAnchor:scroll.contentView.topAnchor],
        [self.phoneRows.widthAnchor constraintEqualToAnchor:scroll.contentView.widthAnchor],
    ]];
    NSView *spacer = [NSView new];
    [stack addArrangedSubview:spacer];
    [spacer.heightAnchor constraintGreaterThanOrEqualToConstant:0].active = YES;
    self.closeButton = [self button:@"关闭" action:@selector(close:)];
    self.closeButton.keyEquivalent = @"\e";
    NSView *footer = [NSView new];
    self.closeButton.translatesAutoresizingMaskIntoConstraints = NO;
    [footer addSubview:self.closeButton];
    [stack addArrangedSubview:footer];
    [footer.widthAnchor constraintEqualToAnchor:stack.widthAnchor].active = YES;
    [footer.heightAnchor constraintEqualToConstant:32].active = YES;
    [NSLayoutConstraint activateConstraints:@[
        [self.closeButton.trailingAnchor constraintEqualToAnchor:footer.trailingAnchor],
        [self.closeButton.centerYAnchor constraintEqualToAnchor:footer.centerYAnchor],
        [self.closeButton.widthAnchor constraintEqualToConstant:80],
        [self.closeButton.heightAnchor constraintEqualToConstant:28],
    ]];
}

- (void)applyState:(NSDictionary *)state {
    self.state = state;
    self.actionPending = NO;
    NSString *uri = HWMobileString(state[@"pairingUri"]);
    if (![self.pairingUri isEqualToString:uri]) {
        self.pairingUri = uri;
        self.qrView.image = HWMobileQRCode(uri);
    }
    self.qrView.hidden = !self.qrView.image;
    self.placeholder.hidden = !!self.qrView.image;
    self.placeholder.stringValue = uri.length ? @"二维码生成失败，请重新生成" :
        ([state[@"linked"] boolValue] ? @"这台 Mac 已绑定，可继续同步" : @"准备好手机后生成配对二维码");
    self.statusLabel.stringValue = HWMobileString(state[@"status"]);
    self.statusLabel.toolTip = self.statusLabel.stringValue;
    self.errorLabel.stringValue = HWMobileString(state[@"error"]);
    self.errorLabel.toolTip = self.errorLabel.stringValue;
    self.generateButton.title = uri.length ? @"重新生成二维码" : @"生成配对二维码";
    for (NSView *row in self.phoneRows.arrangedSubviews.copy) {
        [self.phoneRows removeArrangedSubview:row];
        [row removeFromSuperview];
    }
    self.unlinkButtons = [NSMutableArray new];
    NSArray *phones = [state[@"phones"] isKindOfClass:NSArray.class] ? state[@"phones"] : @[];
    self.phonesLabel.stringValue = [NSString stringWithFormat:@"已绑定手机 (%lu)", (unsigned long)phones.count];
    for (NSDictionary *phone in phones) {
        if (![phone isKindOfClass:NSDictionary.class]) continue;
        NSTextField *name = [NSTextField labelWithString:HWMobileString(phone[@"mobileName"])];
        name.lineBreakMode = NSLineBreakByTruncatingTail;
        name.toolTip = name.stringValue;
        [name setContentCompressionResistancePriority:NSLayoutPriorityDefaultLow forOrientation:NSLayoutConstraintOrientationHorizontal];
        NSButton *unlink = [self button:@"解除绑定" action:@selector(unlink:)];
        unlink.identifier = HWMobileString(phone[@"mobileDeviceId"]);
        [self.unlinkButtons addObject:unlink];
        NSStackView *row = [NSStackView stackViewWithViews:@[name, unlink]];
        row.spacing = 12;
        [self.phoneRows addArrangedSubview:row];
        [row.widthAnchor constraintEqualToAnchor:self.phoneRows.widthAnchor].active = YES;
        [name.widthAnchor constraintGreaterThanOrEqualToConstant:100].active = YES;
    }
    if (!phones.count) {
        [self.phoneRows addArrangedSubview:HWMobileLabel(@"暂无已绑定手机", 12, NSFontWeightRegular)];
    }
    [self updateEnabled];
}

- (void)updateEnabled {
    BOOL enabled = !self.isBusy;
    self.generateButton.enabled = enabled;
    self.refreshButton.enabled = enabled;
    self.closeButton.enabled = enabled;
    [self.panel standardWindowButton:NSWindowCloseButton].enabled = enabled;
    for (NSButton *button in self.unlinkButtons) button.enabled = enabled;
    if (self.isBusy) [self.progress startAnimation:nil];
    else [self.progress stopAnimation:nil];
}

- (void)dispatch:(NSString *)action mobileDeviceId:(NSString *)mobileDeviceId {
    if (self.dismissed || self.isBusy) return;
    self.actionPending = YES;
    [self updateEnabled];
    NSMutableDictionary *detail = [@{ @"panelId": self.panelId, @"action": action } mutableCopy];
    if (mobileDeviceId) detail[@"mobileDeviceId"] = mobileDeviceId;
    NSData *data = [NSJSONSerialization dataWithJSONObject:detail options:0 error:nil];
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    if (!json || !self.webView) return;
    NSString *script = [NSString stringWithFormat:
        @"window.dispatchEvent(new CustomEvent('harmony:native-mobile-bridge-action', {detail:%@}))", json];
    [self.webView evaluateJavaScript:script completionHandler:nil];
}

- (void)generate:(id)sender { [self dispatch:@"generate" mobileDeviceId:nil]; }
- (void)refresh:(id)sender { [self dispatch:@"refresh" mobileDeviceId:nil]; }
- (void)close:(id)sender { [self dispatch:@"close" mobileDeviceId:nil]; }
- (BOOL)windowShouldClose:(NSWindow *)sender { [self close:nil]; return NO; }

- (void)unlink:(NSButton *)sender {
    if (self.isBusy) return;
    NSString *deviceId = sender.identifier;
    NSDictionary *selected = nil;
    for (NSDictionary *phone in self.state[@"phones"]) {
        if ([HWMobileString(phone[@"mobileDeviceId"]) isEqualToString:deviceId]) { selected = phone; break; }
    }
    if (!selected) return;
    self.confirming = YES;
    [self updateEnabled];
    NSAlert *alert = [NSAlert new];
    alert.alertStyle = NSAlertStyleWarning;
    alert.messageText = [NSString stringWithFormat:@"解除与“%@”的绑定？", HWMobileString(selected[@"mobileName"])];
    alert.informativeText = @"该手机将无法继续查看此电脑的会话和任务。";
    [alert addButtonWithTitle:@"解除绑定"];
    [alert addButtonWithTitle:@"取消"];
    [alert beginSheetModalForWindow:self.panel completionHandler:^(NSModalResponse response) {
        self.confirming = NO;
        if (self.dismissed) return;
        if (response == NSAlertFirstButtonReturn) [self dispatch:@"unlink" mobileDeviceId:deviceId];
        [self updateEnabled];
    }];
}

- (void)dismiss {
    self.dismissed = YES;
    if (self.panel.attachedSheet) [self.panel endSheet:self.panel.attachedSheet returnCode:NSModalResponseCancel];
    if (self.panel.sheetParent) [self.parent endSheet:self.panel returnCode:NSModalResponseCancel];
    [self.panel orderOut:nil];
}
@end

bool hw_show_mobile_bridge(void *windowPointer, void *webViewPointer, const char *optionsPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    WKWebView *webView = (__bridge WKWebView *)webViewPointer;
    NSDictionary *options = HWMobileOptions(optionsPointer);
    if (!NSThread.isMainThread || !window || !webView || !options) return false;
    HWMobileBridgeController *previous = objc_getAssociatedObject(window, HWMobileBridgeKey);
    if ([previous.panelId isEqualToString:options[@"panelId"]] && !previous.dismissed) {
        [previous applyState:options[@"state"]];
        return true;
    }
    if (previous) [previous dismiss];
    if (window.attachedSheet) return false;
    HWMobileBridgeController *controller = [HWMobileBridgeController new];
    controller.parent = window;
    controller.webView = webView;
    controller.panelId = options[@"panelId"];
    [controller build];
    [controller applyState:options[@"state"]];
    objc_setAssociatedObject(window, HWMobileBridgeKey, controller, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    [window beginSheet:controller.panel completionHandler:^(NSModalResponse response) {
        (void)response;
        if (!controller.dismissed) [controller dispatch:@"close" mobileDeviceId:nil];
        if (objc_getAssociatedObject(window, HWMobileBridgeKey) == controller) {
            objc_setAssociatedObject(window, HWMobileBridgeKey, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        }
    }];
    return true;
}

bool hw_update_mobile_bridge(void *windowPointer, const char *optionsPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    NSDictionary *options = HWMobileOptions(optionsPointer);
    if (!NSThread.isMainThread || !window || !options) return false;
    HWMobileBridgeController *controller = objc_getAssociatedObject(window, HWMobileBridgeKey);
    if (!controller || controller.dismissed || ![controller.panelId isEqualToString:options[@"panelId"]]) return false;
    [controller applyState:options[@"state"]];
    return true;
}

void hw_close_mobile_bridge(void *windowPointer, const char *panelIdPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    if (!NSThread.isMainThread || !window || !panelIdPointer) return;
    HWMobileBridgeController *controller = objc_getAssociatedObject(window, HWMobileBridgeKey);
    if (![controller.panelId isEqualToString:[NSString stringWithUTF8String:panelIdPointer]]) return;
    [controller dismiss];
    if (objc_getAssociatedObject(window, HWMobileBridgeKey) == controller) {
        objc_setAssociatedObject(window, HWMobileBridgeKey, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
}
