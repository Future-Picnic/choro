#import <Foundation/Foundation.h>
#import <math.h>
#import <objc/runtime.h>

typedef void (*ChoroSparkleEventCallback)(
    void *context,
    int event,
    const char *primary,
    const char *secondary,
    unsigned long long value
);

enum {
    ChoroSparkleEventManualCheckStarted = 1,
    ChoroSparkleEventUpdateFound = 2,
    ChoroSparkleEventNoUpdate = 3,
    ChoroSparkleEventError = 4,
    ChoroSparkleEventDownloadStarted = 5,
    ChoroSparkleEventExpectedLength = 6,
    ChoroSparkleEventReceivedData = 7,
    ChoroSparkleEventExtracting = 8,
    ChoroSparkleEventExtractionProgress = 9,
    ChoroSparkleEventReady = 10,
    ChoroSparkleEventInstalling = 11,
    ChoroSparkleEventDismissed = 12,
    ChoroSparkleEventUpdateFoundReady = 13,
};

typedef NS_ENUM(NSInteger, ChoroSparkleChoice) {
    ChoroSparkleChoiceSkip = 0,
    ChoroSparkleChoiceInstall = 1,
    ChoroSparkleChoiceDismiss = 2,
};

typedef NS_ENUM(NSInteger, ChoroSparkleUserUpdateStage) {
    ChoroSparkleUserUpdateStageNotDownloaded = 0,
    ChoroSparkleUserUpdateStageDownloaded = 1,
    ChoroSparkleUserUpdateStageInstalling = 2,
};

// Sparkle is loaded from the application bundle at runtime so local development
// builds do not need to link against the framework. These declarations give ARC
// the method signatures it needs while keeping the bridge dynamically loaded.
@interface NSObject (ChoroSparkleDynamicAPI)
- (instancetype)initWithAutomaticUpdateChecks:(BOOL)automaticChecks sendSystemProfile:(BOOL)sendSystemProfile;
- (instancetype)initWithHostBundle:(NSBundle *)hostBundle
                 applicationBundle:(NSBundle *)applicationBundle
                         userDriver:(id)userDriver
                           delegate:(nullable id)delegate;
- (void)setFeedURL:(NSURL *)feedURL;
- (void)checkForUpdates;
- (void)checkForUpdatesInBackground;
@end

static const char *ChoroUTF8(NSString *value) {
    return value == nil ? "" : value.UTF8String;
}

@interface ChoroSparkleUserDriver : NSObject
@property(nonatomic, assign) ChoroSparkleEventCallback callback;
@property(nonatomic, assign) void *callbackContext;
@property(nonatomic, copy, nullable) void (^offerReply)(NSInteger);
@property(nonatomic, copy, nullable) void (^readyReply)(NSInteger);
@property(nonatomic, copy, nullable) void (^cancellation)(void);
@property(nonatomic, assign) BOOL manualCheck;
- (void)emit:(int)event primary:(nullable NSString *)primary secondary:(nullable NSString *)secondary value:(uint64_t)value;
@end

@implementation ChoroSparkleUserDriver

- (void)emit:(int)event primary:(NSString *)primary secondary:(NSString *)secondary value:(uint64_t)value {
    if (self.callback != NULL) {
        self.callback(self.callbackContext, event, ChoroUTF8(primary), ChoroUTF8(secondary), value);
    }
}

- (void)showUpdatePermissionRequest:(id)request reply:(void (^)(id))reply {
    (void)request;
    Class responseClass = NSClassFromString(@"SUUpdatePermissionResponse");
    id response = [[responseClass alloc] initWithAutomaticUpdateChecks:YES sendSystemProfile:NO];
    reply(response);
}

- (void)showUserInitiatedUpdateCheckWithCancellation:(void (^)(void))cancellation {
    self.manualCheck = YES;
    self.cancellation = cancellation;
    [self emit:ChoroSparkleEventManualCheckStarted primary:nil secondary:nil value:0];
}

- (void)showUpdateFoundWithAppcastItem:(id)item state:(id)state reply:(void (^)(NSInteger))reply {
    NSString *displayVersion = [item valueForKey:@"displayVersionString"];
    NSString *buildVersion = [item valueForKey:@"versionString"];
    NSString *notes = [item valueForKey:@"itemDescription"];
    NSNumber *length = [item valueForKey:@"contentLength"];
    NSNumber *userInitiated = [state valueForKey:@"userInitiated"];
    NSNumber *stage = [state valueForKey:@"stage"];
    self.manualCheck = userInitiated.boolValue;
    BOOL alreadyInstalling = stage.integerValue == ChoroSparkleUserUpdateStageInstalling;
    if (alreadyInstalling) {
        // Sparkle's Install reply at this stage is a fast quit/relaunch. Hold
        // it as the ready callback so Choro can save and stop processes first.
        self.offerReply = nil;
        self.readyReply = reply;
    } else {
        self.readyReply = nil;
        self.offerReply = reply;
    }
    [self emit:alreadyInstalling ? ChoroSparkleEventUpdateFoundReady : ChoroSparkleEventUpdateFound
          primary:displayVersion
        secondary:[NSString stringWithFormat:@"%@\n%@", buildVersion ?: @"", notes ?: @""]
            value:length.unsignedLongLongValue];
}

- (void)showUpdateReleaseNotesWithDownloadData:(id)data { (void)data; }

- (void)showUpdateReleaseNotesFailedToDownloadWithError:(NSError *)error { (void)error; }

- (void)showUpdateNotFoundWithError:(NSError *)error acknowledgement:(void (^)(void))acknowledgement {
    [self emit:ChoroSparkleEventNoUpdate primary:error.localizedDescription secondary:nil value:self.manualCheck ? 1 : 0];
    self.manualCheck = NO;
    self.cancellation = nil;
    acknowledgement();
}

- (void)showUpdaterError:(NSError *)error acknowledgement:(void (^)(void))acknowledgement {
    [self emit:ChoroSparkleEventError
          primary:error.localizedDescription
        secondary:error.localizedRecoverySuggestion
            value:self.manualCheck ? 1 : 0];
    self.manualCheck = NO;
    self.cancellation = nil;
    acknowledgement();
}

- (void)showDownloadInitiatedWithCancellation:(void (^)(void))cancellation {
    self.cancellation = cancellation;
    [self emit:ChoroSparkleEventDownloadStarted primary:nil secondary:nil value:0];
}

- (void)showDownloadDidReceiveExpectedContentLength:(uint64_t)length {
    [self emit:ChoroSparkleEventExpectedLength primary:nil secondary:nil value:length];
}

- (void)showDownloadDidReceiveDataOfLength:(uint64_t)length {
    [self emit:ChoroSparkleEventReceivedData primary:nil secondary:nil value:length];
}

- (void)showDownloadDidStartExtractingUpdate {
    self.cancellation = nil;
    [self emit:ChoroSparkleEventExtracting primary:nil secondary:nil value:0];
}

- (void)showExtractionReceivedProgress:(double)progress {
    uint64_t scaled = (uint64_t)llround(fmax(0.0, fmin(1.0, progress)) * 10000.0);
    [self emit:ChoroSparkleEventExtractionProgress primary:nil secondary:nil value:scaled];
}

- (void)showReadyToInstallAndRelaunch:(void (^)(NSInteger))reply {
    self.readyReply = reply;
    [self emit:ChoroSparkleEventReady primary:nil secondary:nil value:0];
}

- (void)showInstallingUpdateWithApplicationTerminated:(BOOL)terminated retryTerminatingApplication:(void (^)(void))retry {
    (void)retry;
    [self emit:ChoroSparkleEventInstalling primary:nil secondary:nil value:terminated ? 1 : 0];
}

- (void)showUpdateInstalledAndRelaunched:(BOOL)relaunched acknowledgement:(void (^)(void))acknowledgement {
    (void)relaunched;
    acknowledgement();
}

- (void)dismissUpdateInstallation {
    self.offerReply = nil;
    self.readyReply = nil;
    self.cancellation = nil;
    self.manualCheck = NO;
    [self emit:ChoroSparkleEventDismissed primary:nil secondary:nil value:0];
}

- (void)showUpdateInFocus {
    // Choro's persistent card is already visible; there is no separate window to raise.
}

@end

@interface ChoroSparkleBridge : NSObject
@property(nonatomic, strong) NSBundle *frameworkBundle;
@property(nonatomic, strong) id updater;
@property(nonatomic, strong) ChoroSparkleUserDriver *driver;
@end

@implementation ChoroSparkleBridge
@end

static NSString *ChoroString(const char *value) {
    return value == NULL ? @"" : [NSString stringWithUTF8String:value];
}

static char *ChoroCopyError(NSString *message) {
    return strdup(ChoroUTF8(message.length == 0 ? @"Unknown Sparkle error" : message));
}

void *choro_sparkle_create(
    const char *feedURL,
    const char *token,
    ChoroSparkleEventCallback callback,
    void *callbackContext,
    BOOL manualStart,
    char **errorOut
) {
    @autoreleasepool {
        NSString *frameworkPath = NSProcessInfo.processInfo.environment[@"CHORO_SPARKLE_FRAMEWORK_PATH"];
        if (frameworkPath.length == 0) {
            frameworkPath = [NSBundle.mainBundle.privateFrameworksPath stringByAppendingPathComponent:@"Sparkle.framework"];
        }
        NSBundle *frameworkBundle = [NSBundle bundleWithPath:frameworkPath];
        NSError *loadError = nil;
        if (frameworkBundle == nil || ![frameworkBundle loadAndReturnError:&loadError]) {
            if (errorOut != NULL) {
                NSString *message = loadError.localizedDescription ?: [NSString stringWithFormat:@"Sparkle.framework is unavailable at %@", frameworkPath];
                *errorOut = ChoroCopyError(message);
            }
            return NULL;
        }

        Class updaterClass = NSClassFromString(@"SPUUpdater");
        if (updaterClass == Nil) {
            if (errorOut != NULL) *errorOut = ChoroCopyError(@"Sparkle loaded without SPUUpdater");
            return NULL;
        }

        ChoroSparkleUserDriver *driver = [ChoroSparkleUserDriver new];
        Protocol *userDriverProtocol = objc_getProtocol("SPUUserDriver");
        if (userDriverProtocol != nil) {
            class_addProtocol(ChoroSparkleUserDriver.class, userDriverProtocol);
        }
        driver.callback = callback;
        driver.callbackContext = callbackContext;

        id updater = [[updaterClass alloc]
            initWithHostBundle:NSBundle.mainBundle
            applicationBundle:NSBundle.mainBundle
            userDriver:driver
            delegate:nil];
        if (updater == nil) {
            if (errorOut != NULL) *errorOut = ChoroCopyError(@"Sparkle could not create its updater");
            return NULL;
        }

        NSString *authorization = [NSString stringWithFormat:@"Bearer %@", ChoroString(token)];
        [updater setValue:@{
            @"Authorization": authorization,
            @"Accept": @"application/octet-stream",
            @"X-GitHub-Api-Version": @"2022-11-28",
        } forKey:@"httpHeaders"];
        NSString *feedOverride = ChoroString(feedURL);
        if (feedOverride.length > 0 && NSProcessInfo.processInfo.environment[@"CHORO_UPDATE_FEED_URL"] != nil) {
            NSURL *url = [NSURL URLWithString:feedOverride];
            [updater setFeedURL:url];
        }

        NSError *startError = nil;
        BOOL started = ((BOOL (*)(id, SEL, NSError **))[updater methodForSelector:NSSelectorFromString(@"startUpdater:")])(
            updater,
            NSSelectorFromString(@"startUpdater:"),
            &startError
        );
        if (!started) {
            if (errorOut != NULL) *errorOut = ChoroCopyError(startError.localizedDescription);
            return NULL;
        }

        ChoroSparkleBridge *bridge = [ChoroSparkleBridge new];
        bridge.frameworkBundle = frameworkBundle;
        bridge.updater = updater;
        bridge.driver = driver;
        if (manualStart) {
            [updater checkForUpdates];
        } else {
            [updater checkForUpdatesInBackground];
        }
        return (__bridge_retained void *)bridge;
    }
}

void choro_sparkle_destroy(void *rawBridge) {
    if (rawBridge != NULL) {
        id releasedBridge = CFBridgingRelease(rawBridge);
        (void)releasedBridge;
    }
}

void choro_sparkle_check(void *rawBridge) {
    ChoroSparkleBridge *bridge = (__bridge ChoroSparkleBridge *)rawBridge;
    [bridge.updater checkForUpdates];
}

BOOL choro_sparkle_download(void *rawBridge) {
    ChoroSparkleBridge *bridge = (__bridge ChoroSparkleBridge *)rawBridge;
    if (bridge.driver.offerReply == nil) return NO;
    void (^reply)(NSInteger) = bridge.driver.offerReply;
    bridge.driver.offerReply = nil;
    reply(ChoroSparkleChoiceInstall);
    return YES;
}

BOOL choro_sparkle_dismiss_offer(void *rawBridge) {
    ChoroSparkleBridge *bridge = (__bridge ChoroSparkleBridge *)rawBridge;
    if (bridge.driver.offerReply == nil) return NO;
    void (^reply)(NSInteger) = bridge.driver.offerReply;
    bridge.driver.offerReply = nil;
    reply(ChoroSparkleChoiceDismiss);
    return YES;
}

BOOL choro_sparkle_cancel_download(void *rawBridge) {
    ChoroSparkleBridge *bridge = (__bridge ChoroSparkleBridge *)rawBridge;
    if (bridge.driver.cancellation == nil) return NO;
    void (^cancellation)(void) = bridge.driver.cancellation;
    bridge.driver.cancellation = nil;
    cancellation();
    return YES;
}

BOOL choro_sparkle_reply_ready(void *rawBridge, NSInteger choice) {
    ChoroSparkleBridge *bridge = (__bridge ChoroSparkleBridge *)rawBridge;
    if (bridge.driver.readyReply == nil) return NO;
    if (choice != ChoroSparkleChoiceSkip && choice != ChoroSparkleChoiceInstall) return NO;
    void (^reply)(NSInteger) = bridge.driver.readyReply;
    bridge.driver.readyReply = nil;
    reply(choice);
    return YES;
}

NSInteger choro_bundle_installation_status(void) {
    @autoreleasepool {
        NSString *bundlePath = NSBundle.mainBundle.bundlePath;
        if (bundlePath.length == 0 || ![bundlePath.pathExtension.lowercaseString isEqualToString:@"app"]) {
            return 3;
        }
        if ([bundlePath containsString:@"/AppTranslocation/"]) {
            return 1;
        }

        NSURL *bundleURL = [NSURL fileURLWithPath:bundlePath isDirectory:YES];
        NSNumber *volumeReadOnly = nil;
        if ([bundleURL getResourceValue:&volumeReadOnly forKey:NSURLVolumeIsReadOnlyKey error:nil]
            && volumeReadOnly.boolValue) {
            return 2;
        }

        NSFileManager *fileManager = NSFileManager.defaultManager;
        NSString *parentPath = bundlePath.stringByDeletingLastPathComponent;
        BOOL directlyWritable = [fileManager isWritableFileAtPath:bundlePath]
            || [fileManager isWritableFileAtPath:parentPath];
        NSString *systemApplicationsPrefix = @"/Applications/";
        NSString *userApplicationsPrefix = [[NSHomeDirectory()
            stringByAppendingPathComponent:@"Applications"] stringByAppendingString:@"/"];
        BOOL authorizationSupported = [bundlePath hasPrefix:systemApplicationsPrefix]
            || [bundlePath hasPrefix:userApplicationsPrefix];
        if (!directlyWritable && !authorizationSupported) {
            return 4;
        }
        return 0;
    }
}

char *choro_bundle_short_version(void) {
    NSString *version = [NSBundle.mainBundle objectForInfoDictionaryKey:@"CFBundleShortVersionString"];
    return strdup(ChoroUTF8(version ?: @"Development"));
}

char *choro_bundle_path(void) {
    return strdup(ChoroUTF8(NSBundle.mainBundle.bundlePath));
}

void choro_sparkle_free_string(char *value) {
    free(value);
}
