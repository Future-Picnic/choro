#![allow(unexpected_cfgs)]

use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use ide_core::{CompletionNotifications, NotificationSettings, ProjectId};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttentionCategory {
    NeedsAction,
    Completed,
    Failed,
}

impl AttentionCategory {
    fn identifier(self) -> &'static str {
        match self {
            Self::NeedsAction => "action",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct AttentionKey {
    project_id: ProjectId,
    agent_id: Uuid,
    category: AttentionCategory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionEvent {
    key: AttentionKey,
    pub revision: String,
    pub created_at: u64,
    pub agent_title: String,
    pub project_name: String,
}

impl AttentionEvent {
    pub fn new(
        project_id: ProjectId,
        agent_id: Uuid,
        category: AttentionCategory,
        revision: impl Into<String>,
        created_at: u64,
        agent_title: impl Into<String>,
        project_name: impl Into<String>,
    ) -> Self {
        Self {
            key: AttentionKey {
                project_id,
                agent_id,
                category,
            },
            revision: revision.into(),
            created_at,
            agent_title: agent_title.into(),
            project_name: project_name.into(),
        }
    }

    pub fn project_id(&self) -> ProjectId {
        self.key.project_id
    }

    pub fn agent_id(&self) -> Uuid {
        self.key.agent_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleConversation {
    pub project_id: ProjectId,
    pub agent_id: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationRoute {
    pub project_id: ProjectId,
    pub agent_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanionAttention {
    pub project_id: ProjectId,
    pub agent_id: Uuid,
    pub category: AttentionCategory,
    pub revision: String,
    pub created_at: u64,
    pub agent_title: String,
    pub project_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationPermission {
    Unknown,
    NotDetermined,
    Allowed,
    Provisional,
    Denied,
    Unavailable,
}

impl NotificationPermission {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Checking macOS notification access…",
            Self::NotDetermined => "macOS will ask when the first notification is needed.",
            Self::Allowed => "Notifications are allowed in macOS.",
            Self::Provisional => "Notifications are delivered quietly by macOS.",
            Self::Denied => {
                "Notifications are off in macOS. Choro’s in-app attention state still works."
            }
            Self::Unavailable => "System notifications are unavailable on this platform.",
        }
    }
}

#[derive(Clone, Debug)]
struct AttentionState {
    event: AttentionEvent,
    unread: bool,
    delivered_revision: Option<String>,
    companion_seen_revision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum NotificationAction {
    Deliver {
        identifier: String,
        title: String,
        body: String,
        sound: bool,
    },
    Remove(String),
}

#[derive(Default)]
struct NotificationCoordinator {
    attention: HashMap<AttentionKey, AttentionState>,
    /// Exact maintenance revisions acknowledged before (or after) the regular
    /// attention poll observes them. Consuming an exact revision avoids
    /// suppressing the agent's next real response.
    suppressed_revisions: HashSet<(AttentionKey, String)>,
    primed: bool,
}

impl NotificationCoordinator {
    #[cfg(test)]
    fn synchronize(
        &mut self,
        events: Vec<AttentionEvent>,
        visible: Option<VisibleConversation>,
        app_active: bool,
        preferences: NotificationSettings,
    ) -> Vec<NotificationAction> {
        self.synchronize_with_companion(events, visible, app_active, preferences, false)
    }

    fn synchronize_with_companion(
        &mut self,
        events: Vec<AttentionEvent>,
        visible: Option<VisibleConversation>,
        app_active: bool,
        preferences: NotificationSettings,
        companion_enabled: bool,
    ) -> Vec<NotificationAction> {
        let mut actions = Vec::new();
        let current_keys: HashSet<_> = events.iter().map(|event| event.key.clone()).collect();

        let stale_keys: Vec<_> = self
            .attention
            .keys()
            .filter(|key| !current_keys.contains(*key))
            .cloned()
            .collect();
        for key in stale_keys {
            if let Some(state) = self.attention.remove(&key) {
                actions.push(NotificationAction::Remove(notification_identifier(
                    &state.event,
                )));
            }
        }

        for event in events {
            let suppressed_revision = (event.key.clone(), event.revision.clone());
            let suppressed = self.suppressed_revisions.remove(&suppressed_revision);
            let exact_agent_visible = app_active
                && visible.is_some_and(|visible| {
                    visible.project_id == event.project_id() && visible.agent_id == event.agent_id()
                });
            let state = self
                .attention
                .entry(event.key.clone())
                .or_insert_with(|| AttentionState {
                    delivered_revision: (!self.primed).then(|| event.revision.clone()),
                    companion_seen_revision: None,
                    event: event.clone(),
                    unread: self.primed || event.key.category == AttentionCategory::NeedsAction,
                });

            if state.event.revision != event.revision {
                state.unread = true;
                state.delivered_revision = None;
            }
            state.event = event;

            if suppressed {
                state.unread = false;
                state.delivered_revision = Some(state.event.revision.clone());
                state.companion_seen_revision = Some(state.event.revision.clone());
                actions.push(NotificationAction::Remove(notification_identifier(
                    &state.event,
                )));
                continue;
            }

            if exact_agent_visible {
                state.unread = false;
                state.delivered_revision = Some(state.event.revision.clone());
                state.companion_seen_revision = Some(state.event.revision.clone());
                actions.push(NotificationAction::Remove(notification_identifier(
                    &state.event,
                )));
                continue;
            }

            // The companion is the notification surface while it is visible.
            // Forget a previously delivered banner revision so an unread item
            // can be surfaced if the user later turns the companion off.
            if companion_enabled {
                state.delivered_revision = None;
                actions.push(NotificationAction::Remove(notification_identifier(
                    &state.event,
                )));
                continue;
            }

            let enabled = if state.event.key.category == AttentionCategory::NeedsAction {
                preferences.questions_and_approvals
            } else {
                match preferences.completion {
                    CompletionNotifications::Never => false,
                    CompletionNotifications::Background => !app_active,
                    CompletionNotifications::Always => true,
                }
            };
            if !enabled || !state.unread {
                if !enabled {
                    actions.push(NotificationAction::Remove(notification_identifier(
                        &state.event,
                    )));
                }
                continue;
            }
            if state.delivered_revision.as_deref() == Some(state.event.revision.as_str()) {
                continue;
            }

            let (title, body) = notification_copy(&state.event);
            actions.push(NotificationAction::Deliver {
                identifier: notification_identifier(&state.event),
                title,
                body,
                sound: state.event.key.category == AttentionCategory::NeedsAction
                    && preferences.sound,
            });
            state.delivered_revision = Some(state.event.revision.clone());
        }

        self.primed = true;
        actions
    }

    fn suppress_revision(
        &mut self,
        project_id: ProjectId,
        agent_id: Uuid,
        category: AttentionCategory,
        revision: String,
    ) -> Vec<NotificationAction> {
        let key = AttentionKey {
            project_id,
            agent_id,
            category,
        };
        let revision_key = (key.clone(), revision.clone());
        self.suppressed_revisions.insert(revision_key.clone());

        let mut actions = Vec::new();
        if let Some(state) = self
            .attention
            .get_mut(&key)
            .filter(|state| state.event.revision == revision)
        {
            state.unread = false;
            state.delivered_revision = Some(state.event.revision.clone());
            state.companion_seen_revision = Some(state.event.revision.clone());
            self.suppressed_revisions.remove(&revision_key);
            actions.push(NotificationAction::Remove(notification_identifier(
                &state.event,
            )));
        }
        actions
    }

    fn acknowledge_agent(
        &mut self,
        project_id: ProjectId,
        agent_id: Uuid,
    ) -> Vec<NotificationAction> {
        let mut actions = Vec::new();
        for state in self.attention.values_mut().filter(|state| {
            state.event.project_id() == project_id && state.event.agent_id() == agent_id
        }) {
            state.unread = false;
            state.delivered_revision = Some(state.event.revision.clone());
            state.companion_seen_revision = Some(state.event.revision.clone());
            actions.push(NotificationAction::Remove(notification_identifier(
                &state.event,
            )));
        }
        actions
    }

    fn unread_agent_count(&self) -> usize {
        self.attention
            .values()
            .filter(|state| state.unread)
            .map(|state| (state.event.project_id(), state.event.agent_id()))
            .collect::<HashSet<_>>()
            .len()
    }

    fn companion_attention(&self) -> Vec<CompanionAttention> {
        let mut attention = self
            .attention
            .values()
            .filter(|state| {
                state.companion_seen_revision.as_deref() != Some(state.event.revision.as_str())
            })
            .map(|state| CompanionAttention {
                project_id: state.event.project_id(),
                agent_id: state.event.agent_id(),
                category: state.event.key.category,
                revision: state.event.revision.clone(),
                created_at: state.event.created_at,
                agent_title: state.event.agent_title.clone(),
                project_name: state.event.project_name.clone(),
            })
            .collect::<Vec<_>>();
        attention.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.agent_title.cmp(&b.agent_title))
        });
        attention
    }
}

fn coordinator() -> &'static Mutex<NotificationCoordinator> {
    static COORDINATOR: OnceLock<Mutex<NotificationCoordinator>> = OnceLock::new();
    COORDINATOR.get_or_init(|| Mutex::new(NotificationCoordinator::default()))
}

fn notification_identifier(event: &AttentionEvent) -> String {
    format!(
        "choro:{}:{}:{}",
        event.project_id().0,
        event.agent_id(),
        event.key.category.identifier()
    )
}

fn notification_copy(event: &AttentionEvent) -> (String, String) {
    let title = match event.key.category {
        AttentionCategory::NeedsAction => "Agent needs attention",
        AttentionCategory::Completed => "Agent finished",
        AttentionCategory::Failed => "Agent stopped",
    };
    (
        title.to_string(),
        format!("{} · {}", event.agent_title, event.project_name),
    )
}

fn parse_notification_route(identifier: &str) -> Option<NotificationRoute> {
    let mut parts = identifier.split(':');
    if parts.next()? != "choro" {
        return None;
    }
    let project_id = ProjectId(Uuid::parse_str(parts.next()?).ok()?);
    let agent_id = Uuid::parse_str(parts.next()?).ok()?;
    let category = parts.next()?;
    if parts.next().is_some() || !matches!(category, "action" | "completed" | "failed") {
        return None;
    }
    Some(NotificationRoute {
        project_id,
        agent_id,
    })
}

pub fn initialize() {
    platform::initialize();
}

pub fn synchronize(
    events: Vec<AttentionEvent>,
    visible: Option<VisibleConversation>,
    preferences: NotificationSettings,
    companion_enabled: bool,
) {
    let actions = coordinator()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .synchronize_with_companion(
            events,
            visible,
            platform::is_app_active(),
            preferences,
            companion_enabled,
        );
    execute_actions(actions);
    update_dock_badge();
}

pub fn acknowledge_agent(project_id: ProjectId, agent_id: Uuid) {
    let actions = coordinator()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .acknowledge_agent(project_id, agent_id);
    execute_actions(actions);
    update_dock_badge();
}

pub fn suppress_agent_attention_revision(
    project_id: ProjectId,
    agent_id: Uuid,
    category: AttentionCategory,
    revision: String,
) {
    let actions = coordinator()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .suppress_revision(project_id, agent_id, category, revision);
    execute_actions(actions);
    update_dock_badge();
}

pub fn unread_agent_count() -> usize {
    coordinator()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .unread_agent_count()
}

pub fn companion_attention() -> Vec<CompanionAttention> {
    coordinator()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .companion_attention()
}

pub fn take_clicked_route() -> Option<NotificationRoute> {
    platform::take_clicked_route()
}

pub fn permission() -> NotificationPermission {
    platform::initialize();
    platform::permission()
}

pub fn open_system_notification_settings() {
    platform::open_system_notification_settings();
}

fn execute_actions(actions: Vec<NotificationAction>) {
    for action in actions {
        match action {
            NotificationAction::Deliver {
                identifier,
                title,
                body,
                sound,
            } => platform::deliver(&identifier, &title, &body, sound),
            NotificationAction::Remove(identifier) => platform::remove(&identifier),
        }
    }
}

fn update_dock_badge() {
    let count = unread_agent_count();
    let label = (count > 0).then(|| count.to_string());
    set_dock_badge(label.as_deref());
}

pub fn play_generated_sound() {
    let _ = Command::new("afplay")
        .arg("/System/Library/Sounds/Glass.aiff")
        .spawn();
}

#[cfg(target_os = "macos")]
fn ns_string(value: &str) -> *mut objc::runtime::Object {
    unsafe {
        use objc::{class, msg_send, sel, sel_impl};

        let string: *mut objc::runtime::Object = msg_send![class!(NSString), alloc];
        let string: *mut objc::runtime::Object =
            msg_send![string, initWithBytes:value.as_ptr() length:value.len() encoding:4usize];
        let string: *mut objc::runtime::Object = msg_send![string, autorelease];
        string
    }
}

#[cfg(target_os = "macos")]
pub fn set_dock_badge(label: Option<&str>) {
    unsafe {
        use objc::runtime::Object;
        use objc::{class, msg_send, sel, sel_impl};

        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let dock_tile: *mut Object = msg_send![app, dockTile];
        let label = label.filter(|value| !value.trim().is_empty());
        let badge: *mut Object = label.map(ns_string).unwrap_or(std::ptr::null_mut());
        let _: () = msg_send![dock_tile, setBadgeLabel:badge];
        let _: () = msg_send![dock_tile, display];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_badge(_label: Option<&str>) {}

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread};
    use objc2_foundation::{NSArray, NSError, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    use super::{parse_notification_route, NotificationPermission, NotificationRoute};

    static INITIALIZED: OnceLock<()> = OnceLock::new();
    static REQUESTED_PERMISSION: AtomicBool = AtomicBool::new(false);
    static PERMISSION_STATE: OnceLock<Mutex<PermissionCallbackState>> = OnceLock::new();
    static CLICKED_ROUTE: OnceLock<Mutex<Option<NotificationRoute>>> = OnceLock::new();
    static PENDING_DELIVERIES: OnceLock<Mutex<Vec<PendingDelivery>>> = OnceLock::new();
    static LAST_PERMISSION_REFRESH: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    static DELEGATE: OnceLock<Retained<NotificationDelegate>> = OnceLock::new();

    const PERMISSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

    #[derive(Default)]
    struct PermissionCallbackState {
        generation: u64,
        value: u8,
    }

    impl PermissionCallbackState {
        fn begin(&mut self) -> u64 {
            self.generation = self.generation.wrapping_add(1);
            self.generation
        }

        fn complete(&mut self, generation: u64, value: u8) -> bool {
            if self.generation != generation {
                return false;
            }
            self.value = value;
            true
        }
    }

    #[derive(Clone)]
    struct PendingDelivery {
        identifier: String,
        title: String,
        body: String,
        sound: bool,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "ChoroUserNotificationDelegate"]
        #[thread_kind = AnyThread]
        #[ivars = ()]
        struct NotificationDelegate;

        unsafe impl NSObjectProtocol for NotificationDelegate {}

        unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                completion_handler.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion_handler: &block2::DynBlock<dyn Fn()>,
            ) {
                let identifier = response.notification().request().identifier().to_string();
                if let Some(route) = parse_notification_route(&identifier) {
                    *clicked_route()
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(route);
                    activate_app();
                }
                completion_handler.call(());
            }
        }
    );

    fn clicked_route() -> &'static Mutex<Option<NotificationRoute>> {
        CLICKED_ROUTE.get_or_init(|| Mutex::new(None))
    }

    fn pending_deliveries() -> &'static Mutex<Vec<PendingDelivery>> {
        PENDING_DELIVERIES.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn queue_pending_delivery(payload: PendingDelivery) {
        let mut pending = pending_deliveries()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        pending.retain(|item| item.identifier != payload.identifier);
        pending.push(payload);
    }

    fn remove_pending_delivery(identifier: &str) {
        pending_deliveries()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|item| item.identifier != identifier);
    }

    fn clear_pending_deliveries() {
        pending_deliveries()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }

    fn deliver_pending_if_authorized(status: UNAuthorizationStatus) {
        match status {
            UNAuthorizationStatus::Authorized
            | UNAuthorizationStatus::Ephemeral
            | UNAuthorizationStatus::Provisional => {
                let pending = std::mem::take(
                    &mut *pending_deliveries()
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()),
                );
                for payload in pending {
                    deliver_authorized(
                        &payload.identifier,
                        &payload.title,
                        &payload.body,
                        payload.sound,
                    );
                }
            }
            UNAuthorizationStatus::Denied => clear_pending_deliveries(),
            _ => {}
        }
    }

    fn permission_state() -> &'static Mutex<PermissionCallbackState> {
        PERMISSION_STATE.get_or_init(|| Mutex::new(PermissionCallbackState::default()))
    }

    fn begin_permission_update() -> u64 {
        permission_state()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .begin()
    }

    pub fn initialize() {
        INITIALIZED.get_or_init(|| {
            let delegate = DELEGATE.get_or_init(|| {
                let delegate = NotificationDelegate::alloc().set_ivars(());
                unsafe { msg_send![super(delegate), init] }
            });
            let center = UNUserNotificationCenter::currentNotificationCenter();
            center.setDelegate(Some(ProtocolObject::from_ref(&**delegate)));

            refresh_permission();
            cleanup_legacy_notifications();
        });
    }

    fn refresh_permission() {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let generation = begin_permission_update();
        let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let settings = unsafe { settings.as_ref() };
            let status = settings.authorizationStatus();
            if set_permission(generation, status) {
                deliver_pending_if_authorized(status);
            }
        });
        center.getNotificationSettingsWithCompletionHandler(&handler);
    }

    fn refresh_denied_permission_if_stale() {
        let now = Instant::now();
        let should_refresh = {
            let mut last_refresh = LAST_PERMISSION_REFRESH
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if last_refresh
                .is_some_and(|last| now.duration_since(last) < PERMISSION_REFRESH_INTERVAL)
            {
                false
            } else {
                *last_refresh = Some(now);
                true
            }
        };
        if should_refresh {
            refresh_permission();
        }
    }

    fn set_permission(generation: u64, status: UNAuthorizationStatus) -> bool {
        let value = match status {
            UNAuthorizationStatus::NotDetermined => 1,
            UNAuthorizationStatus::Authorized | UNAuthorizationStatus::Ephemeral => 2,
            UNAuthorizationStatus::Provisional => 3,
            UNAuthorizationStatus::Denied => 4,
            _ => 5,
        };
        permission_state()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .complete(generation, value)
    }

    fn cached_permission() -> NotificationPermission {
        let value = permission_state()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .value;
        match value {
            1 => NotificationPermission::NotDetermined,
            2 => NotificationPermission::Allowed,
            3 => NotificationPermission::Provisional,
            4 => NotificationPermission::Denied,
            5 => NotificationPermission::Unavailable,
            _ => NotificationPermission::Unknown,
        }
    }

    pub fn permission() -> NotificationPermission {
        let permission = cached_permission();
        if permission == NotificationPermission::Denied {
            refresh_denied_permission_if_stale();
        }
        permission
    }

    pub fn is_app_active() -> bool {
        unsafe {
            use objc::{class, msg_send, sel, sel_impl};
            let app: *mut objc::runtime::Object =
                msg_send![class!(NSApplication), sharedApplication];
            let active: bool = msg_send![app, isActive];
            active
        }
    }

    pub fn deliver(identifier: &str, title: &str, body: &str, sound: bool) {
        initialize();
        let payload = PendingDelivery {
            identifier: identifier.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            sound,
        };
        match cached_permission() {
            NotificationPermission::Allowed | NotificationPermission::Provisional => {
                deliver_authorized(
                    &payload.identifier,
                    &payload.title,
                    &payload.body,
                    payload.sound,
                );
            }
            NotificationPermission::Denied => {
                queue_pending_delivery(payload);
                // A delivery attempt must always re-check the OS state. The
                // throttled settings refresh may have completed just before
                // the user enabled notifications, and this event must not be
                // left queued indefinitely in that race.
                refresh_permission();
            }
            NotificationPermission::Unavailable => {}
            NotificationPermission::Unknown | NotificationPermission::NotDetermined => {
                queue_pending_delivery(payload);
                if REQUESTED_PERMISSION.swap(true, Ordering::Relaxed) {
                    return;
                }
                let center = UNUserNotificationCenter::currentNotificationCenter();
                let generation = begin_permission_update();
                let handler = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
                    let value = if granted.as_bool() { 2 } else { 4 };
                    let applied = permission_state()
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .complete(generation, value);
                    if !applied {
                        return;
                    }
                    if granted.as_bool() {
                        deliver_pending_if_authorized(UNAuthorizationStatus::Authorized);
                    } else {
                        clear_pending_deliveries();
                    }
                });
                center.requestAuthorizationWithOptions_completionHandler(
                    UNAuthorizationOptions::Alert
                        | UNAuthorizationOptions::Badge
                        | UNAuthorizationOptions::Sound,
                    &handler,
                );
            }
        }
    }

    fn deliver_authorized(identifier: &str, title: &str, body: &str, sound: bool) {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setThreadIdentifier(&NSString::from_str("choro-agent-attention"));
        if sound {
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(identifier),
            &content,
            None,
        );
        center.addNotificationRequest_withCompletionHandler(&request, None);
    }

    pub fn remove(identifier: &str) {
        initialize();
        remove_pending_delivery(identifier);
        let identifier = NSString::from_str(identifier);
        let identifiers = NSArray::from_slice(&[&*identifier]);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        center.removePendingNotificationRequestsWithIdentifiers(&identifiers);
        center.removeDeliveredNotificationsWithIdentifiers(&identifiers);
    }

    pub fn take_clicked_route() -> Option<NotificationRoute> {
        clicked_route()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }

    pub fn open_system_notification_settings() {
        let _ = Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.notifications")
            .spawn();
    }

    fn activate_app() {
        unsafe {
            use objc::{class, msg_send, sel, sel_impl};
            let app: *mut objc::runtime::Object =
                msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, activateIgnoringOtherApps:true];
        }
    }

    fn cleanup_legacy_notifications() {
        unsafe {
            use objc::{class, msg_send, sel, sel_impl};
            let center: *mut objc::runtime::Object = msg_send![
                class!(NSUserNotificationCenter),
                defaultUserNotificationCenter
            ];
            if !center.is_null() {
                let _: () = msg_send![center, removeAllDeliveredNotifications];
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            queue_pending_delivery, remove_pending_delivery, PendingDelivery,
            PermissionCallbackState,
        };

        #[test]
        fn older_permission_callback_cannot_overwrite_newer_answer() {
            let mut state = PermissionCallbackState::default();
            let settings_query = state.begin();
            let authorization_request = state.begin();

            assert!(!state.complete(settings_query, 1));
            assert!(state.complete(authorization_request, 2));
            assert_eq!(state.value, 2);
        }

        #[test]
        fn late_denial_from_previous_request_is_ignored() {
            let mut state = PermissionCallbackState::default();
            let previous = state.begin();
            let current = state.begin();

            assert!(state.complete(current, 2));
            assert!(!state.complete(previous, 4));
            assert_eq!(state.value, 2);
        }

        #[test]
        fn removing_a_notification_also_cancels_its_queued_delivery() {
            let identifier = "queued-notification";
            queue_pending_delivery(PendingDelivery {
                identifier: identifier.to_string(),
                title: "Agent".to_string(),
                body: "Needs attention".to_string(),
                sound: true,
            });

            remove_pending_delivery(identifier);

            assert!(super::pending_deliveries()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .all(|payload| payload.identifier != identifier));
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{NotificationPermission, NotificationRoute};

    pub fn initialize() {}
    pub fn is_app_active() -> bool {
        true
    }
    pub fn deliver(_identifier: &str, _title: &str, _body: &str, _sound: bool) {}
    pub fn remove(_identifier: &str) {}
    pub fn take_clicked_route() -> Option<NotificationRoute> {
        None
    }
    pub fn permission() -> NotificationPermission {
        NotificationPermission::Unavailable
    }
    pub fn open_system_notification_settings() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(agent_id: Uuid, category: AttentionCategory, revision: &str) -> AttentionEvent {
        AttentionEvent::new(
            ProjectId(Uuid::nil()),
            agent_id,
            category,
            revision,
            10,
            "Agent",
            "Project",
        )
    }

    fn background_preferences() -> NotificationSettings {
        NotificationSettings::default()
    }

    #[test]
    fn unchanged_poll_is_idempotent_and_state_is_bounded() {
        let agent = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        assert!(coordinator
            .synchronize(vec![], None, false, background_preferences())
            .is_empty());

        let first = coordinator.synchronize(
            vec![event(agent, AttentionCategory::NeedsAction, "question-1")],
            None,
            false,
            background_preferences(),
        );
        assert_eq!(
            first
                .iter()
                .filter(|action| matches!(action, NotificationAction::Deliver { .. }))
                .count(),
            1
        );
        assert!(coordinator
            .synchronize(
                vec![event(agent, AttentionCategory::NeedsAction, "question-1")],
                None,
                false,
                background_preferences(),
            )
            .is_empty());
        assert_eq!(coordinator.attention.len(), 1);

        coordinator.synchronize(vec![], None, false, background_preferences());
        assert!(coordinator.attention.is_empty());
    }

    #[test]
    fn viewing_agent_acknowledges_only_that_agent_and_allows_new_revision() {
        let project = ProjectId(Uuid::nil());
        let viewed = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(vec![], None, false, background_preferences());
        coordinator.synchronize(
            vec![
                event(viewed, AttentionCategory::NeedsAction, "question-1"),
                event(other, AttentionCategory::NeedsAction, "question-1"),
            ],
            None,
            false,
            background_preferences(),
        );

        coordinator.synchronize(
            vec![
                event(viewed, AttentionCategory::NeedsAction, "question-1"),
                event(other, AttentionCategory::NeedsAction, "question-1"),
            ],
            Some(VisibleConversation {
                project_id: project,
                agent_id: viewed,
            }),
            true,
            background_preferences(),
        );
        assert_eq!(coordinator.unread_agent_count(), 1);

        let actions = coordinator.synchronize(
            vec![
                event(viewed, AttentionCategory::NeedsAction, "question-2"),
                event(other, AttentionCategory::NeedsAction, "question-1"),
            ],
            None,
            false,
            background_preferences(),
        );
        assert!(actions
            .iter()
            .any(|action| matches!(action, NotificationAction::Deliver { .. })));
        assert_eq!(coordinator.unread_agent_count(), 2);
    }

    #[test]
    fn exact_visible_conversation_suppresses_a_new_notification() {
        let project = ProjectId(Uuid::nil());
        let agent = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(vec![], None, false, background_preferences());

        let actions = coordinator.synchronize(
            vec![event(agent, AttentionCategory::NeedsAction, "question-1")],
            Some(VisibleConversation {
                project_id: project,
                agent_id: agent,
            }),
            true,
            background_preferences(),
        );

        assert!(actions
            .iter()
            .all(|action| !matches!(action, NotificationAction::Deliver { .. })));
        assert_eq!(coordinator.unread_agent_count(), 0);
    }

    #[test]
    fn multiple_unread_categories_for_one_agent_count_once() {
        let agent = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(vec![], None, false, background_preferences());
        coordinator.synchronize(
            vec![
                event(agent, AttentionCategory::NeedsAction, "question-1"),
                event(agent, AttentionCategory::Completed, "turn-1"),
            ],
            None,
            false,
            background_preferences(),
        );

        assert_eq!(coordinator.unread_agent_count(), 1);
    }

    #[test]
    fn companion_attention_contains_all_unseen_attention_states() {
        let action_agent = Uuid::new_v4();
        let completed_agent = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(vec![], None, false, background_preferences());
        coordinator.synchronize(
            vec![
                event(action_agent, AttentionCategory::NeedsAction, "question-1"),
                event(completed_agent, AttentionCategory::Completed, "turn-1"),
            ],
            None,
            false,
            background_preferences(),
        );

        let attention = coordinator.companion_attention();
        assert_eq!(attention.len(), 2);
        assert!(attention.iter().any(|item| {
            item.agent_id == action_agent
                && item.category == AttentionCategory::NeedsAction
                && item.revision == "question-1"
        }));
        assert!(attention.iter().any(|item| {
            item.agent_id == completed_agent
                && item.category == AttentionCategory::Completed
                && item.revision == "turn-1"
        }));

        coordinator.acknowledge_agent(ProjectId(Uuid::nil()), action_agent);
        let attention = coordinator.companion_attention();
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].agent_id, completed_agent);
    }

    #[test]
    fn exact_maintenance_completion_is_hidden_but_the_next_completion_is_not() {
        let project = ProjectId(Uuid::nil());
        let agent = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(vec![], None, false, background_preferences());

        assert!(coordinator
            .suppress_revision(
                project,
                agent,
                AttentionCategory::Completed,
                "completed:summary".to_string(),
            )
            .is_empty());
        let actions = coordinator.synchronize(
            vec![event(
                agent,
                AttentionCategory::Completed,
                "completed:summary",
            )],
            None,
            false,
            background_preferences(),
        );
        assert!(actions
            .iter()
            .all(|action| !matches!(action, NotificationAction::Deliver { .. })));
        assert!(coordinator.companion_attention().is_empty());

        coordinator.synchronize(
            vec![event(
                agent,
                AttentionCategory::Completed,
                "completed:real-work",
            )],
            None,
            false,
            background_preferences(),
        );
        let attention = coordinator.companion_attention();
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].revision, "completed:real-work");
    }

    #[test]
    fn maintenance_completion_can_be_hidden_after_attention_polling() {
        let project = ProjectId(Uuid::nil());
        let agent = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(vec![], None, false, background_preferences());
        coordinator.synchronize(
            vec![event(
                agent,
                AttentionCategory::Completed,
                "completed:summary",
            )],
            None,
            false,
            background_preferences(),
        );
        assert_eq!(coordinator.companion_attention().len(), 1);

        coordinator.suppress_revision(
            project,
            agent,
            AttentionCategory::Completed,
            "completed:summary".to_string(),
        );
        assert!(coordinator.companion_attention().is_empty());
    }

    #[test]
    fn visible_conversation_is_hidden_until_its_attention_revision_changes() {
        let project_id = ProjectId(Uuid::nil());
        let agent_id = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(
            vec![event(agent_id, AttentionCategory::Completed, "turn-1")],
            Some(VisibleConversation {
                project_id,
                agent_id,
            }),
            true,
            background_preferences(),
        );
        assert!(coordinator.companion_attention().is_empty());

        coordinator.synchronize(
            vec![event(agent_id, AttentionCategory::Completed, "turn-2")],
            None,
            false,
            background_preferences(),
        );
        assert_eq!(coordinator.companion_attention().len(), 1);
    }

    #[test]
    fn completion_policy_is_background_only_and_silent_by_default() {
        let agent = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(vec![], None, false, background_preferences());
        assert!(coordinator
            .synchronize(
                vec![event(agent, AttentionCategory::Completed, "turn-1")],
                None,
                true,
                background_preferences(),
            )
            .iter()
            .all(|action| !matches!(action, NotificationAction::Deliver { .. })));

        let actions = coordinator.synchronize(
            vec![event(agent, AttentionCategory::Completed, "turn-2")],
            None,
            false,
            background_preferences(),
        );
        assert!(actions
            .iter()
            .any(|action| matches!(action, NotificationAction::Deliver { sound: false, .. })));
    }

    #[test]
    fn companion_replaces_banners_and_unread_work_returns_when_hidden() {
        let agent = Uuid::new_v4();
        let mut coordinator = NotificationCoordinator::default();
        coordinator.synchronize(vec![], None, false, background_preferences());
        let events = vec![event(agent, AttentionCategory::NeedsAction, "question-1")];

        let while_visible = coordinator.synchronize_with_companion(
            events.clone(),
            None,
            false,
            background_preferences(),
            true,
        );
        assert!(while_visible
            .iter()
            .all(|action| !matches!(action, NotificationAction::Deliver { .. })));
        assert_eq!(coordinator.companion_attention().len(), 1);

        let after_hiding = coordinator.synchronize_with_companion(
            events.clone(),
            None,
            false,
            background_preferences(),
            false,
        );
        assert!(after_hiding
            .iter()
            .any(|action| matches!(action, NotificationAction::Deliver { .. })));

        let after_showing = coordinator.synchronize_with_companion(
            events.clone(),
            None,
            false,
            background_preferences(),
            true,
        );
        assert!(after_showing
            .iter()
            .any(|action| matches!(action, NotificationAction::Remove(_))));

        let after_hiding_again = coordinator.synchronize_with_companion(
            events,
            None,
            false,
            background_preferences(),
            false,
        );
        assert!(after_hiding_again
            .iter()
            .any(|action| matches!(action, NotificationAction::Deliver { .. })));
    }

    #[test]
    fn route_identifier_round_trips_project_and_agent() {
        let project = ProjectId(Uuid::new_v4());
        let agent = Uuid::new_v4();
        let event = AttentionEvent::new(
            project,
            agent,
            AttentionCategory::NeedsAction,
            "r1",
            1,
            "Agent",
            "Project",
        );
        assert_eq!(
            parse_notification_route(&notification_identifier(&event)),
            Some(NotificationRoute {
                project_id: project,
                agent_id: agent,
            })
        );
    }
}
