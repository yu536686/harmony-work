#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import <objc/runtime.h>

static const void *HWSidebarKey = &HWSidebarKey;
static const void *HWToolbarKey = &HWToolbarKey;
static NSToolbarItemIdentifier const HWSidebarItem = @"harmony.sidebar";
static NSToolbarItemIdentifier const HWNewChatItem = @"harmony.new-chat";

@interface HWToolbarController : NSObject <NSToolbarDelegate>
@property (nonatomic, weak) WKWebView *webView;
@end

@implementation HWToolbarController

- (NSArray<NSToolbarItemIdentifier> *)toolbarAllowedItemIdentifiers:(NSToolbar *)toolbar {
    return @[HWSidebarItem, HWNewChatItem, NSToolbarFlexibleSpaceItemIdentifier];
}

- (NSArray<NSToolbarItemIdentifier> *)toolbarDefaultItemIdentifiers:(NSToolbar *)toolbar {
    return @[HWSidebarItem, HWNewChatItem, NSToolbarFlexibleSpaceItemIdentifier];
}

- (NSToolbarItem *)toolbar:(NSToolbar *)toolbar
       itemForItemIdentifier:(NSToolbarItemIdentifier)identifier
   willBeInsertedIntoToolbar:(BOOL)flag {
    NSToolbarItem *item = [[NSToolbarItem alloc] initWithItemIdentifier:identifier];
    item.target = self;
    if ([identifier isEqualToString:HWSidebarItem]) {
        item.label = @"显示或隐藏侧边栏";
        item.toolTip = item.label;
        item.image = [NSImage imageWithSystemSymbolName:@"sidebar.left" accessibilityDescription:item.label];
        item.action = @selector(toggleSidebar:);
    } else if ([identifier isEqualToString:HWNewChatItem]) {
        item.label = @"新对话";
        item.toolTip = item.label;
        item.image = [NSImage imageWithSystemSymbolName:@"square.and.pencil" accessibilityDescription:item.label];
        item.action = @selector(newChat:);
    }
    return item;
}

- (void)toggleSidebar:(id)sender {
    [self.webView evaluateJavaScript:@"window.dispatchEvent(new Event('harmony:native-toggle-sidebar'))"
                completionHandler:nil];
}

- (void)newChat:(id)sender {
    [self.webView evaluateJavaScript:@"window.dispatchEvent(new Event('harmony:native-new-chat'))"
                completionHandler:nil];
}

@end

void hw_install_native_shell(void *windowPointer, void *webViewPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    WKWebView *webView = (__bridge WKWebView *)webViewPointer;
    if (!window || !webView || objc_getAssociatedObject(window, HWToolbarKey)) return;

    window.titleVisibility = NSWindowTitleHidden;
    window.titlebarAppearsTransparent = YES;
    window.toolbarStyle = NSWindowToolbarStyleUnified;

    HWToolbarController *controller = [HWToolbarController new];
    controller.webView = webView;
    NSToolbar *toolbar = [[NSToolbar alloc] initWithIdentifier:@"harmony.main-toolbar"];
    toolbar.delegate = controller;
    toolbar.displayMode = NSToolbarDisplayModeIconOnly;
    toolbar.allowsUserCustomization = NO;
    window.toolbar = toolbar;
    objc_setAssociatedObject(window, HWToolbarKey, controller, OBJC_ASSOCIATION_RETAIN_NONATOMIC);

    NSView *host = webView.superview;
    if (!host) return;
    NSRect frame = NSMakeRect(NSMinX(webView.frame), NSMinY(webView.frame), 280, NSHeight(webView.frame));
    NSView *sidebar;
    Class glassClass = NSClassFromString(@"NSGlassEffectView");
    if (glassClass) {
        NSView *glass = [[glassClass alloc] initWithFrame:frame];
        [glass setValue:@0 forKey:@"style"]; // Regular glass; interactive behavior defaults to off.
        sidebar = glass;
    } else {
        NSVisualEffectView *vibrancy = [[NSVisualEffectView alloc] initWithFrame:frame];
        vibrancy.material = NSVisualEffectMaterialSidebar;
        vibrancy.blendingMode = NSVisualEffectBlendingModeBehindWindow;
        vibrancy.state = NSVisualEffectStateFollowsWindowActiveState;
        sidebar = vibrancy;
    }
    sidebar.autoresizingMask = NSViewHeightSizable;
    [host addSubview:sidebar positioned:NSWindowBelow relativeTo:webView];
    objc_setAssociatedObject(window, HWSidebarKey, sidebar, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}

void hw_set_shell_state(void *windowPointer, double width, bool dark) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    NSView *sidebar = objc_getAssociatedObject(window, HWSidebarKey);
    if (!sidebar) return;
    NSAppearance *appearance = [NSAppearance appearanceNamed:dark ? NSAppearanceNameDarkAqua : NSAppearanceNameAqua];
    window.appearance = appearance;
    sidebar.appearance = appearance;
    NSRect frame = sidebar.frame;
    frame.size.width = MAX(0, MIN(width, 420));
    sidebar.frame = frame;
}
