#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import <objc/runtime.h>

static const void *HWSidebarKey = &HWSidebarKey;
static const void *HWToolbarKey = &HWToolbarKey;
static const void *HWDropdownKey = &HWDropdownKey;
static const void *HWMenuKey = &HWMenuKey;
static const void *HWAnnotationKey = &HWAnnotationKey;
static const void *HWDialogKey = &HWDialogKey;
static const void *HWInfoKey = &HWInfoKey;
static NSToolbarItemIdentifier const HWSidebarItem = @"harmony.sidebar";
static NSToolbarItemIdentifier const HWNewChatItem = @"harmony.new-chat";
static NSToolbarItemIdentifier const HWMobileItem = @"harmony.mobile";
static NSToolbarItemIdentifier const HWUpdateItem = @"harmony.update";

@interface HWToolbarController : NSObject <NSToolbarDelegate>
@property (nonatomic, weak) WKWebView *webView;
@end

@interface HWMenuController : NSObject <NSMenuDelegate>
@property (nonatomic, weak) WKWebView *webView;
@property (nonatomic, copy) NSString *menuId;
@property (nonatomic, strong) NSMenu *menu;
@property (nonatomic) BOOL selected;
@end

@interface HWAnnotationController : NSObject <NSPopoverDelegate, NSTextViewDelegate>
@property (nonatomic, weak) WKWebView *webView;
@property (nonatomic, copy) NSString *annotationId;
@property (nonatomic, strong) NSPopover *popover;
@property (nonatomic, strong) NSTextView *editor;
@property (nonatomic, strong) NSButton *stageButton;
@property (nonatomic) BOOL suppressClose;
@end

@interface HWDialogController : NSObject <NSTextFieldDelegate>
@property (nonatomic, weak) WKWebView *webView;
@property (nonatomic, copy) NSString *dialogId;
@property (nonatomic, strong) NSAlert *alert;
@property (nonatomic, strong) NSTextField *input;
@property (nonatomic) BOOL suppressResult;
@end

@interface HWInfoController : NSObject <NSPopoverDelegate>
@property (nonatomic, weak) WKWebView *webView;
@property (nonatomic, copy) NSString *infoId;
@property (nonatomic, strong) NSPopover *popover;
@property (nonatomic) BOOL suppressClose;
@end

@interface HWFlippedView : NSView
@end

@implementation HWFlippedView
- (BOOL)isFlipped { return YES; }
@end

@class HWDropdownController;
@interface HWDropdownButton : NSButton
@property (nonatomic, weak) HWDropdownController *owner;
@property (nonatomic, strong) NSTrackingArea *hoverTracking;
@end

@interface HWDropdownController : NSObject <NSPopoverDelegate, NSSearchFieldDelegate>
@property (nonatomic, weak) WKWebView *webView;
@property (nonatomic, copy) NSString *menuId;
@property (nonatomic, strong) NSArray<NSDictionary *> *items;
@property (nonatomic, strong) NSPopover *popover;
@property (nonatomic, strong) NSView *root;
@property (nonatomic, strong) NSScrollView *scrollView;
@property (nonatomic, strong) NSView *hoverGlass;
@property (nonatomic, strong) NSSearchField *searchField;
@property (nonatomic, copy) NSString *emptyLabel;
@property (nonatomic) BOOL closeOnSelect;
- (void)renderItems;
- (void)highlightButton:(HWDropdownButton *)button;
- (void)clearHighlight:(HWDropdownButton *)button;
@end

@implementation HWDropdownButton

- (void)updateTrackingAreas {
    if (self.hoverTracking) [self removeTrackingArea:self.hoverTracking];
    self.hoverTracking = [[NSTrackingArea alloc] initWithRect:NSZeroRect
        options:NSTrackingMouseEnteredAndExited | NSTrackingActiveInKeyWindow | NSTrackingInVisibleRect
        owner:self userInfo:nil];
    [self addTrackingArea:self.hoverTracking];
    [super updateTrackingAreas];
}

- (void)mouseEntered:(NSEvent *)event { [self.owner highlightButton:self]; }
- (void)mouseExited:(NSEvent *)event { [self.owner clearHighlight:self]; }

@end

static void HWDispatchDropdownEvent(WKWebView *webView, NSString *name, NSDictionary *detail) {
    if (!webView) return;
    NSData *data = [NSJSONSerialization dataWithJSONObject:detail options:0 error:nil];
    if (!data) return;
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    NSString *script = [NSString stringWithFormat:@"window.dispatchEvent(new CustomEvent('%@', {detail:%@}))", name, json];
    [webView evaluateJavaScript:script completionHandler:nil];
}

@implementation HWMenuController

- (void)selectItem:(NSMenuItem *)sender {
    self.selected = YES;
    HWDispatchDropdownEvent(self.webView, @"harmony:native-menu-select", @{
        @"menuId": self.menuId, @"id": sender.representedObject
    });
}

- (void)menuDidClose:(NSMenu *)menu {
    HWDispatchDropdownEvent(self.webView, @"harmony:native-menu-close", @{ @"menuId": self.menuId });
}

@end

@implementation HWAnnotationController

- (void)textDidChange:(NSNotification *)notification {
    NSString *comment = [self.editor.string stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    self.stageButton.enabled = comment.length > 0;
}

- (void)stage:(id)sender {
    NSString *comment = [self.editor.string stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    if (!comment.length) return;
    self.suppressClose = YES;
    HWDispatchDropdownEvent(self.webView, @"harmony:native-annotation-stage", @{
        @"annotationId": self.annotationId, @"comment": comment
    });
    [self.popover close];
}

- (void)cancel:(id)sender { [self.popover close]; }

- (void)popoverDidClose:(NSNotification *)notification {
    if (!self.suppressClose) {
        HWDispatchDropdownEvent(self.webView, @"harmony:native-annotation-close", @{
            @"annotationId": self.annotationId
        });
    }
}

@end

@implementation HWDialogController

- (void)controlTextDidChange:(NSNotification *)notification {
    NSString *value = [self.input.stringValue stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    self.alert.buttons.firstObject.enabled = value.length > 0;
}

@end

@implementation HWInfoController
- (void)popoverDidClose:(NSNotification *)notification {
    if (!self.suppressClose) {
        HWDispatchDropdownEvent(self.webView, @"harmony:native-info-close", @{ @"infoId": self.infoId });
    }
}
@end

@implementation HWDropdownController

- (void)highlightButton:(HWDropdownButton *)button {
    if (!button.enabled || !self.hoverGlass) return;
    BOOL reduceMotion = NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
    NSRect target = NSInsetRect(button.frame, 1, 1);
    if (self.hoverGlass.alphaValue < 0.01) self.hoverGlass.frame = target;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = reduceMotion ? 0 : 0.18;
        self.hoverGlass.animator.frame = target;
        self.hoverGlass.animator.alphaValue = 1;
    } completionHandler:nil];
}

- (void)clearHighlight:(HWDropdownButton *)button {
    if (!self.hoverGlass) return;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion ? 0 : 0.12;
        self.hoverGlass.animator.alphaValue = 0;
    } completionHandler:nil];
}

- (void)popoverDidClose:(NSNotification *)notification {
    HWDispatchDropdownEvent(self.webView, @"harmony:native-dropdown-close", @{ @"menuId": self.menuId });
}

- (void)controlTextDidChange:(NSNotification *)notification {
    [self renderItems];
}

- (void)selectItem:(NSButton *)sender {
    NSDictionary *item = self.items[(NSUInteger)sender.tag];
    HWDispatchDropdownEvent(self.webView, @"harmony:native-dropdown-select", @{
        @"menuId": self.menuId, @"id": item[@"id"]
    });
    if (self.closeOnSelect) [self.popover close];
}

- (void)renderItems {
    NSString *query = self.searchField.stringValue.lowercaseString ?: @"";
    NSMutableArray<NSNumber *> *indices = [NSMutableArray array];
    for (NSUInteger index = 0; index < self.items.count; index++) {
        NSString *label = self.items[index][@"label"];
        if (!query.length || [label.lowercaseString containsString:query]) {
            [indices addObject:@(index)];
        }
    }

    CGFloat width = 280;
    CGFloat listHeight = MIN(MAX(indices.count, 1), 7) * 31 + 8;
    CGFloat searchHeight = self.searchField ? 42 : 0;
    CGFloat height = listHeight + searchHeight + 12;
    HWFlippedView *document = [[HWFlippedView alloc] initWithFrame:NSMakeRect(0, 0, width - 18, MAX(indices.count, 1) * 31 + 8)];
    BOOL opaque = NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceTransparency;
    Class glassClass = opaque ? Nil : NSClassFromString(@"NSGlassEffectView");
    if (glassClass) {
        self.hoverGlass = [[glassClass alloc] initWithFrame:NSZeroRect];
        [self.hoverGlass setValue:@0 forKey:@"style"];
        [self.hoverGlass setValue:@YES forKey:@"effectIsInteractive"];
        [self.hoverGlass setValue:@6 forKey:@"cornerRadius"];
    } else {
        self.hoverGlass = [[NSView alloc] initWithFrame:NSZeroRect];
        self.hoverGlass.wantsLayer = YES;
        self.hoverGlass.layer.backgroundColor = NSColor.selectedControlColor.CGColor;
        self.hoverGlass.layer.cornerRadius = 6;
    }
    self.hoverGlass.alphaValue = 0;
    [document addSubview:self.hoverGlass];
    if (!indices.count) {
        NSTextField *empty = [NSTextField labelWithString:self.emptyLabel ?: @"无结果"];
        empty.textColor = NSColor.secondaryLabelColor;
        empty.frame = NSMakeRect(10, 7, width - 38, 24);
        [document addSubview:empty];
    }
    for (NSUInteger row = 0; row < indices.count; row++) {
        NSUInteger index = indices[row].unsignedIntegerValue;
        NSDictionary *item = self.items[index];
        HWDropdownButton *button = [[HWDropdownButton alloc] initWithFrame:NSMakeRect(4, row * 31 + 4, width - 28, 28)];
        button.owner = self;
        button.title = item[@"label"];
        button.alignment = NSTextAlignmentLeft;
        button.font = [NSFont systemFontOfSize:13];
        button.bordered = NO;
        button.imagePosition = NSImageRight;
        button.tag = (NSInteger)index;
        button.target = self;
        button.action = @selector(selectItem:);
        button.enabled = ![item[@"disabled"] boolValue];
        if ([item[@"selected"] boolValue]) {
            button.image = [NSImage imageWithSystemSymbolName:@"checkmark" accessibilityDescription:@"已选择"];
            button.state = NSControlStateValueOn;
        }
        if ([item[@"action"] boolValue]) button.contentTintColor = NSColor.controlAccentColor;
        button.toolTip = item[@"label"];
        [document addSubview:button];
    }
    self.scrollView.documentView = document;
    self.scrollView.frame = NSMakeRect(8, 7, width - 16, listHeight);
    if (self.searchField) self.searchField.frame = NSMakeRect(12, listHeight + 11, width - 24, 28);
    self.root.frame = NSMakeRect(0, 0, width, height);
    self.popover.contentSize = NSMakeSize(width, height);
}

@end

@implementation HWToolbarController

- (NSArray<NSToolbarItemIdentifier> *)toolbarAllowedItemIdentifiers:(NSToolbar *)toolbar {
    return @[HWSidebarItem, HWNewChatItem, NSToolbarFlexibleSpaceItemIdentifier, HWMobileItem, HWUpdateItem];
}

- (NSArray<NSToolbarItemIdentifier> *)toolbarDefaultItemIdentifiers:(NSToolbar *)toolbar {
    return @[HWSidebarItem, HWNewChatItem, NSToolbarFlexibleSpaceItemIdentifier, HWMobileItem, HWUpdateItem];
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
    } else if ([identifier isEqualToString:HWMobileItem]) {
        item.label = @"连接手机";
        item.toolTip = item.label;
        item.image = [NSImage imageWithSystemSymbolName:@"iphone.and.arrow.forward" accessibilityDescription:item.label];
        if (!item.image) item.image = [NSImage imageWithSystemSymbolName:@"iphone" accessibilityDescription:item.label];
        item.action = @selector(connectPhone:);
    } else if ([identifier isEqualToString:HWUpdateItem]) {
        item.label = @"检查更新";
        item.toolTip = item.label;
        item.image = [NSImage imageWithSystemSymbolName:@"arrow.down.circle" accessibilityDescription:item.label];
        item.action = @selector(checkUpdates:);
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

- (void)connectPhone:(id)sender {
    [self.webView evaluateJavaScript:@"window.dispatchEvent(new Event('harmony:native-mobile-bridge'))" completionHandler:nil];
}

- (void)checkUpdates:(id)sender {
    [self.webView evaluateJavaScript:@"window.dispatchEvent(new Event('harmony:native-check-updates'))" completionHandler:nil];
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
    NSMenu *appMenu = NSApp.mainMenu.itemArray.firstObject.submenu;
    if (appMenu && ![appMenu itemWithTitle:@"检查更新…"]) {
        NSMenuItem *update = [[NSMenuItem alloc] initWithTitle:@"检查更新…" action:@selector(checkUpdates:) keyEquivalent:@""];
        update.target = controller;
        [appMenu insertItem:update atIndex:MIN((NSInteger)2, appMenu.numberOfItems)];
    }

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
    window.backgroundColor = dark
        ? [NSColor colorWithCalibratedRed:0.10 green:0.11 blue:0.14 alpha:1]
        : [NSColor windowBackgroundColor];
    if ([sidebar respondsToSelector:@selector(setTintColor:)]) {
        NSColor *tint = dark
            ? [NSColor colorWithCalibratedRed:0.10 green:0.11 blue:0.14 alpha:0.70]
            : nil;
        [sidebar setValue:tint forKey:@"tintColor"];
    }
    NSRect frame = sidebar.frame;
    frame.size.width = MAX(0, MIN(width, 420));
    sidebar.frame = frame;
}

bool hw_show_dropdown(void *windowPointer, void *webViewPointer, const char *optionsPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    WKWebView *webView = (__bridge WKWebView *)webViewPointer;
    if (!window || !webView || !optionsPointer) return false;
    NSData *data = [[NSString stringWithUTF8String:optionsPointer] dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *options = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![options isKindOfClass:NSDictionary.class] || ![options[@"items"] isKindOfClass:NSArray.class]) return false;

    HWDropdownController *previous = objc_getAssociatedObject(window, HWDropdownKey);
    [previous.popover close];

    HWDropdownController *controller = [HWDropdownController new];
    controller.webView = webView;
    controller.menuId = options[@"menuId"];
    controller.items = options[@"items"];
    controller.closeOnSelect = [options[@"closeOnSelect"] boolValue];
    controller.emptyLabel = [options[@"emptyLabel"] isKindOfClass:NSString.class] ? options[@"emptyLabel"] : @"无结果";
    controller.popover = [NSPopover new];
    controller.popover.behavior = NSPopoverBehaviorTransient;
    controller.popover.animates = !NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
    controller.popover.delegate = controller;

    NSViewController *content = [NSViewController new];
    controller.root = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 280, 220)];
    controller.scrollView = [[NSScrollView alloc] initWithFrame:NSZeroRect];
    controller.scrollView.drawsBackground = NO;
    controller.scrollView.hasVerticalScroller = YES;
    controller.scrollView.autohidesScrollers = YES;
    [controller.root addSubview:controller.scrollView];
    NSString *placeholder = [options[@"searchPlaceholder"] isKindOfClass:NSString.class] ? options[@"searchPlaceholder"] : nil;
    BOOL allowSearch = ![options[@"showSearch"] isKindOfClass:NSNumber.class] || [options[@"showSearch"] boolValue];
    if (allowSearch && (placeholder.length || controller.items.count >= 6)) {
        controller.searchField = [[NSSearchField alloc] initWithFrame:NSZeroRect];
        controller.searchField.placeholderString = placeholder ?: @"搜索";
        controller.searchField.delegate = controller;
        [controller.root addSubview:controller.searchField];
    }
    content.view = controller.root;
    controller.popover.contentViewController = content;
    [controller renderItems];
    objc_setAssociatedObject(window, HWDropdownKey, controller, OBJC_ASSOCIATION_RETAIN_NONATOMIC);

    double x = [options[@"x"] doubleValue];
    double y = [options[@"y"] doubleValue];
    double width = [options[@"width"] doubleValue];
    double height = [options[@"height"] doubleValue];
    NSRect anchor = NSMakeRect(x, webView.isFlipped ? y : NSHeight(webView.bounds) - y - height, width, height);
    BOOL above = [options[@"placement"] isEqualToString:@"above"];
    [controller.popover showRelativeToRect:anchor ofView:webView preferredEdge:above ? NSRectEdgeMaxY : NSRectEdgeMinY];
    if (controller.searchField) [controller.popover.contentViewController.view.window makeFirstResponder:controller.searchField];
    return controller.popover.isShown;
}

void hw_close_dropdown(void *windowPointer, const char *menuIdPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    HWDropdownController *controller = objc_getAssociatedObject(window, HWDropdownKey);
    if (controller && menuIdPointer && [controller.menuId isEqualToString:[NSString stringWithUTF8String:menuIdPointer]]) {
        [controller.popover close];
    }
}

void hw_update_dropdown(void *windowPointer, const char *menuIdPointer, const char *itemsPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    HWDropdownController *controller = objc_getAssociatedObject(window, HWDropdownKey);
    if (!controller || !controller.popover.isShown || !menuIdPointer || !itemsPointer) return;
    if (![controller.menuId isEqualToString:[NSString stringWithUTF8String:menuIdPointer]]) return;
    NSData *data = [[NSString stringWithUTF8String:itemsPointer] dataUsingEncoding:NSUTF8StringEncoding];
    NSArray *items = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![items isKindOfClass:NSArray.class]) return;
    controller.items = items;
    [controller renderItems];
}

bool hw_show_menu(void *windowPointer, void *webViewPointer, const char *optionsPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    WKWebView *webView = (__bridge WKWebView *)webViewPointer;
    if (!window || !webView || !optionsPointer) return false;
    NSData *data = [[NSString stringWithUTF8String:optionsPointer] dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *options = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![options isKindOfClass:NSDictionary.class] || ![options[@"items"] isKindOfClass:NSArray.class]) return false;

    HWMenuController *controller = [HWMenuController new];
    controller.webView = webView;
    controller.menuId = options[@"menuId"];
    controller.menu = [[NSMenu alloc] initWithTitle:@""];
    controller.menu.delegate = controller;
    controller.menu.autoenablesItems = NO;
    for (NSDictionary *item in options[@"items"]) {
        if ([item[@"separator"] boolValue]) {
            [controller.menu addItem:NSMenuItem.separatorItem];
            continue;
        }
        if (![item[@"label"] isKindOfClass:NSString.class] || ![item[@"id"] isKindOfClass:NSString.class]) continue;
        NSMenuItem *row = [[NSMenuItem alloc] initWithTitle:item[@"label"] action:@selector(selectItem:) keyEquivalent:@""];
        row.target = controller;
        row.representedObject = item[@"id"];
        row.enabled = ![item[@"disabled"] boolValue];
        if ([item[@"selected"] boolValue]) row.state = NSControlStateValueOn;
        [controller.menu addItem:row];
    }
    if (!controller.menu.numberOfItems) return false;
    objc_setAssociatedObject(window, HWMenuKey, controller, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    CGFloat x = [options[@"x"] doubleValue];
    CGFloat y = [options[@"y"] doubleValue];
    NSPoint point = NSMakePoint(x, webView.isFlipped ? y : NSHeight(webView.bounds) - y);
    [controller.menu popUpMenuPositioningItem:nil atLocation:point inView:webView];
    return YES;
}

bool hw_show_annotation(void *windowPointer, void *webViewPointer, const char *optionsPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    WKWebView *webView = (__bridge WKWebView *)webViewPointer;
    if (!window || !webView || !optionsPointer) return false;
    NSData *data = [[NSString stringWithUTF8String:optionsPointer] dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *options = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![options isKindOfClass:NSDictionary.class]) return false;

    HWAnnotationController *previous = objc_getAssociatedObject(window, HWAnnotationKey);
    previous.suppressClose = YES;
    [previous.popover close];

    HWAnnotationController *controller = [HWAnnotationController new];
    controller.webView = webView;
    controller.annotationId = options[@"annotationId"];
    controller.popover = [NSPopover new];
    controller.popover.behavior = NSPopoverBehaviorTransient;
    controller.popover.animates = !NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
    controller.popover.delegate = controller;
    controller.popover.contentSize = NSMakeSize(320, 193);

    NSView *root = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 320, 193)];
    NSTextField *heading = [NSTextField labelWithString:options[@"title"] ?: @"添加批注"];
    heading.font = [NSFont boldSystemFontOfSize:13];
    heading.frame = NSMakeRect(14, 160, 290, 22);
    [root addSubview:heading];

    NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(14, 50, 292, 100)];
    scroll.hasVerticalScroller = YES;
    scroll.autohidesScrollers = YES;
    scroll.borderType = NSBezelBorder;
    controller.editor = [[NSTextView alloc] initWithFrame:scroll.bounds];
    controller.editor.font = [NSFont systemFontOfSize:13];
    controller.editor.string = [options[@"comment"] isKindOfClass:NSString.class] ? options[@"comment"] : @"";
    controller.editor.delegate = controller;
    controller.editor.minSize = NSMakeSize(0, scroll.contentSize.height);
    controller.editor.maxSize = NSMakeSize(CGFLOAT_MAX, CGFLOAT_MAX);
    controller.editor.verticallyResizable = YES;
    controller.editor.horizontallyResizable = NO;
    controller.editor.autoresizingMask = NSViewWidthSizable;
    controller.editor.textContainer.widthTracksTextView = YES;
    scroll.documentView = controller.editor;
    [root addSubview:scroll];

    NSButton *cancel = [NSButton buttonWithTitle:options[@"cancelLabel"] ?: @"取消" target:controller action:@selector(cancel:)];
    cancel.frame = NSMakeRect(152, 12, 74, 29);
    cancel.keyEquivalent = @"\033";
    [root addSubview:cancel];
    controller.stageButton = [NSButton buttonWithTitle:options[@"stageLabel"] ?: @"添加批注" target:controller action:@selector(stage:)];
    controller.stageButton.frame = NSMakeRect(230, 12, 76, 29);
    controller.stageButton.bezelStyle = NSBezelStyleRounded;
    controller.stageButton.keyEquivalent = @"\r";
    controller.stageButton.enabled = controller.editor.string.length > 0;
    [root addSubview:controller.stageButton];

    NSViewController *content = [NSViewController new];
    content.view = root;
    controller.popover.contentViewController = content;
    objc_setAssociatedObject(window, HWAnnotationKey, controller, OBJC_ASSOCIATION_RETAIN_NONATOMIC);

    CGFloat x = [options[@"x"] doubleValue];
    CGFloat y = [options[@"y"] doubleValue];
    NSRect anchor = NSMakeRect(x, webView.isFlipped ? y : NSHeight(webView.bounds) - y - 4, 4, 4);
    [controller.popover showRelativeToRect:anchor ofView:webView preferredEdge:NSRectEdgeMinY];
    [controller.popover.contentViewController.view.window makeFirstResponder:controller.editor];
    return controller.popover.isShown;
}

void hw_close_annotation(void *windowPointer, const char *annotationIdPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    HWAnnotationController *controller = objc_getAssociatedObject(window, HWAnnotationKey);
    if (controller && annotationIdPointer &&
        [controller.annotationId isEqualToString:[NSString stringWithUTF8String:annotationIdPointer]]) {
        controller.suppressClose = YES;
        [controller.popover close];
    }
}

bool hw_show_info(void *windowPointer, void *webViewPointer, const char *optionsPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    WKWebView *webView = (__bridge WKWebView *)webViewPointer;
    if (!window || !webView || !optionsPointer) return false;
    NSData *data = [[NSString stringWithUTF8String:optionsPointer] dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *options = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![options isKindOfClass:NSDictionary.class] || ![options[@"items"] isKindOfClass:NSArray.class]) return false;

    HWInfoController *previous = objc_getAssociatedObject(window, HWInfoKey);
    previous.suppressClose = YES;
    [previous.popover close];
    HWInfoController *controller = [HWInfoController new];
    controller.webView = webView;
    controller.infoId = options[@"infoId"];
    controller.popover = [NSPopover new];
    controller.popover.behavior = NSPopoverBehaviorTransient;
    controller.popover.animates = !NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
    controller.popover.delegate = controller;

    CGFloat width = 310;
    NSArray *items = options[@"items"];
    CGFloat contentHeight = MAX(36, items.count * 48);
    CGFloat visibleHeight = MIN(contentHeight, 270);
    NSView *root = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, width, visibleHeight + 49)];
    NSTextField *heading = [NSTextField labelWithString:options[@"title"] ?: @""];
    heading.font = [NSFont boldSystemFontOfSize:13];
    heading.frame = NSMakeRect(13, visibleHeight + 17, width - 26, 22);
    [root addSubview:heading];
    NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(8, 8, width - 16, visibleHeight)];
    scroll.drawsBackground = NO;
    scroll.hasVerticalScroller = contentHeight > visibleHeight;
    scroll.autohidesScrollers = YES;
    HWFlippedView *document = [[HWFlippedView alloc] initWithFrame:NSMakeRect(0, 0, width - 32, contentHeight)];
    if (items.count == 0) {
        NSTextField *empty = [NSTextField labelWithString:options[@"emptyLabel"] ?: @""];
        empty.textColor = NSColor.secondaryLabelColor;
        empty.frame = NSMakeRect(8, 7, width - 48, 22);
        [document addSubview:empty];
    }
    for (NSUInteger index = 0; index < items.count; index++) {
        NSDictionary *item = items[index];
        if (![item isKindOfClass:NSDictionary.class]) continue;
        CGFloat y = index * 48 + 3;
        NSTextField *label = [NSTextField labelWithString:item[@"label"] ?: @""];
        label.font = [NSFont systemFontOfSize:11 weight:NSFontWeightSemibold];
        label.textColor = NSColor.secondaryLabelColor;
        label.frame = NSMakeRect(8, y, width - 48, 17);
        [document addSubview:label];
        NSTextField *value = [NSTextField labelWithString:item[@"value"] ?: @""];
        value.font = [NSFont systemFontOfSize:12];
        value.lineBreakMode = NSLineBreakByTruncatingTail;
        value.toolTip = item[@"value"];
        value.frame = NSMakeRect(8, y + 18, width - 48, 22);
        [document addSubview:value];
    }
    scroll.documentView = document;
    [root addSubview:scroll];
    NSViewController *content = [NSViewController new];
    content.view = root;
    controller.popover.contentViewController = content;
    controller.popover.contentSize = root.frame.size;
    objc_setAssociatedObject(window, HWInfoKey, controller, OBJC_ASSOCIATION_RETAIN_NONATOMIC);

    CGFloat x = [options[@"x"] doubleValue];
    CGFloat y = [options[@"y"] doubleValue];
    CGFloat anchorWidth = [options[@"width"] doubleValue];
    CGFloat anchorHeight = [options[@"height"] doubleValue];
    NSRect anchor = NSMakeRect(x, webView.isFlipped ? y : NSHeight(webView.bounds) - y - anchorHeight,
                               anchorWidth, anchorHeight);
    [controller.popover showRelativeToRect:anchor ofView:webView preferredEdge:NSRectEdgeMinY];
    return controller.popover.isShown;
}

void hw_close_info(void *windowPointer, const char *infoIdPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    HWInfoController *controller = objc_getAssociatedObject(window, HWInfoKey);
    if (controller && infoIdPointer &&
        [controller.infoId isEqualToString:[NSString stringWithUTF8String:infoIdPointer]]) {
        controller.suppressClose = YES;
        [controller.popover close];
    }
}

bool hw_show_dialog(void *windowPointer, void *webViewPointer, const char *optionsPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    WKWebView *webView = (__bridge WKWebView *)webViewPointer;
    if (!window || !webView || !optionsPointer) return false;
    NSData *data = [[NSString stringWithUTF8String:optionsPointer] dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *options = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![options isKindOfClass:NSDictionary.class]) return false;

    HWDialogController *previous = objc_getAssociatedObject(window, HWDialogKey);
    if (previous.alert.window.sheetParent) {
        previous.suppressResult = YES;
        [window endSheet:previous.alert.window returnCode:NSModalResponseCancel];
    }
    HWDialogController *controller = [HWDialogController new];
    controller.webView = webView;
    controller.dialogId = options[@"dialogId"];
    controller.alert = [NSAlert new];
    controller.alert.messageText = options[@"title"] ?: @"";
    controller.alert.informativeText = options[@"message"] ?: @"";
    controller.alert.alertStyle = [options[@"danger"] boolValue] ? NSAlertStyleWarning : NSAlertStyleInformational;
    [controller.alert addButtonWithTitle:options[@"confirmLabel"] ?: @"确定"];
    if (![options[@"alert"] boolValue]) [controller.alert addButtonWithTitle:options[@"cancelLabel"] ?: @"取消"];
    if ([options[@"extraLabel"] isKindOfClass:NSString.class] && [options[@"extraLabel"] length]) {
        [controller.alert addButtonWithTitle:options[@"extraLabel"]];
    }

    if ([options[@"prompt"] boolValue]) {
        controller.input = [[NSTextField alloc] initWithFrame:NSMakeRect(0, 0, 320, 26)];
        controller.input.stringValue = [options[@"value"] isKindOfClass:NSString.class] ? options[@"value"] : @"";
        controller.input.placeholderString = options[@"placeholder"];
        controller.input.delegate = controller;
        controller.alert.accessoryView = controller.input;
        NSString *trimmed = [controller.input.stringValue stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
        controller.alert.buttons.firstObject.enabled = trimmed.length > 0;
    } else if ([options[@"detail"] isKindOfClass:NSString.class] && [options[@"detail"] length]) {
        NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(0, 0, 420, 150)];
        scroll.hasVerticalScroller = YES;
        scroll.autohidesScrollers = YES;
        scroll.borderType = NSBezelBorder;
        NSTextView *detail = [[NSTextView alloc] initWithFrame:scroll.bounds];
        detail.string = options[@"detail"];
        detail.font = [NSFont monospacedSystemFontOfSize:11 weight:NSFontWeightRegular];
        detail.editable = NO;
        detail.selectable = YES;
        detail.verticallyResizable = YES;
        detail.autoresizingMask = NSViewWidthSizable;
        detail.textContainer.widthTracksTextView = YES;
        scroll.documentView = detail;
        controller.alert.accessoryView = scroll;
    }

    objc_setAssociatedObject(window, HWDialogKey, controller, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    [controller.alert beginSheetModalForWindow:window completionHandler:^(NSModalResponse response) {
        if (!controller.suppressResult) {
            HWDispatchDropdownEvent(controller.webView, @"harmony:native-dialog-result", @{
                @"dialogId": controller.dialogId,
                @"accepted": @(response == NSAlertFirstButtonReturn),
                @"extra": @(response == NSAlertThirdButtonReturn),
                @"value": controller.input.stringValue ?: @"",
            });
        }
        if (objc_getAssociatedObject(window, HWDialogKey) == controller) {
            objc_setAssociatedObject(window, HWDialogKey, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        }
    }];
    if (controller.input) [controller.alert.window makeFirstResponder:controller.input];
    return YES;
}

void hw_close_dialog(void *windowPointer, const char *dialogIdPointer) {
    NSWindow *window = (__bridge NSWindow *)windowPointer;
    HWDialogController *controller = objc_getAssociatedObject(window, HWDialogKey);
    if (controller && dialogIdPointer &&
        [controller.dialogId isEqualToString:[NSString stringWithUTF8String:dialogIdPointer]] &&
        controller.alert.window.sheetParent) {
        controller.suppressResult = YES;
        [window endSheet:controller.alert.window returnCode:NSModalResponseCancel];
    }
}
