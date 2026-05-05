use super::{Client, proto};
use anyhow::Result;
use collections::HashMap;
use gpui::{App, Context, EventEmitter, SharedString, SharedUri, Task};
use http_client::http::{HeaderMap, HeaderValue};
use postage::watch;
use rpc::TypedEnvelope;
use std::sync::Arc;
use text::ReplicaId;

pub type UserId = u64;

#[derive(
    Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, serde::Serialize, serde::Deserialize,
)]
pub struct ChannelId(pub u64);

impl std::fmt::Display for ChannelId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct ProjectId(pub u64);

impl ProjectId {
    pub fn to_proto(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantIndex(pub u32);

#[derive(Default, Debug)]
pub struct User {
    pub id: UserId,
    pub github_login: SharedString,
    pub avatar_uri: SharedUri,
    pub name: Option<String>,
}

impl PartialOrd for User {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for User {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.github_login.cmp(&other.github_login)
    }
}

impl PartialEq for User {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.github_login == other.github_login
    }
}

impl Eq for User {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collaborator {
    pub peer_id: proto::PeerId,
    pub replica_id: ReplicaId,
    pub user_id: UserId,
    pub is_host: bool,
    pub committer_name: Option<String>,
    pub committer_email: Option<String>,
}

impl Collaborator {
    pub fn from_proto(message: proto::Collaborator) -> Result<Self> {
        Ok(Self {
            peer_id: message.peer_id.unwrap_or_default(),
            replica_id: ReplicaId::new(message.replica_id as u16),
            user_id: message.user_id as UserId,
            is_host: message.is_host,
            committer_name: message.committer_name,
            committer_email: message.committer_email,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct Contact {
    pub user: Arc<User>,
    pub online: bool,
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactRequestStatus {
    None,
    RequestSent,
    RequestReceived,
    RequestAccepted,
}

pub struct UserStore {
    users: HashMap<u64, Arc<User>>,
    participant_indices: HashMap<u64, ParticipantIndex>,
    current_user: watch::Receiver<Option<Arc<User>>>,
}

pub enum Event {
    Contact {
        user: Arc<User>,
        kind: ContactEventKind,
    },
    ShowContacts,
    ParticipantIndicesChanged,
    PrivateUserInfoUpdated,
    PlanUpdated,
    OrganizationChanged,
}

#[derive(Clone, Copy)]
pub enum ContactEventKind {
    Requested,
    Accepted,
    Cancelled,
}

impl EventEmitter<Event> for UserStore {}

#[derive(Clone)]
pub struct InviteInfo {
    pub count: u32,
    pub url: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationId(pub Arc<str>);

#[derive(Debug, Default)]
pub struct Organization {
    pub id: OrganizationId,
    pub name: SharedString,
}

impl Default for OrganizationId {
    fn default() -> Self {
        Self(Arc::from(""))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Plan {
    Free,
}

#[derive(Debug, Clone)]
pub struct PlanInfo {
    pub plan: Plan,
}

#[derive(Debug, Clone, Default)]
pub struct OrganizationConfiguration {
    pub edit_prediction: OrganizationEditPredictionConfiguration,
}

#[derive(Debug, Clone)]
pub struct OrganizationEditPredictionConfiguration {
    pub is_enabled: bool,
}

impl Default for OrganizationEditPredictionConfiguration {
    fn default() -> Self {
        Self { is_enabled: false }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UsageLimit {
    Limited(i32),
    Unlimited,
}

#[derive(Debug, Clone, Copy)]
pub struct RequestUsage {
    pub limit: UsageLimit,
    pub amount: i32,
}

impl RequestUsage {
    pub fn over_limit(&self) -> bool {
        match self.limit {
            UsageLimit::Limited(limit) => self.amount >= limit,
            UsageLimit::Unlimited => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EditPredictionUsage(pub RequestUsage);

impl std::ops::Deref for EditPredictionUsage {
    type Target = RequestUsage;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl EditPredictionUsage {
    pub fn from_headers(_: &HeaderMap<HeaderValue>) -> Result<Self> {
        Ok(Self(RequestUsage {
            limit: UsageLimit::Unlimited,
            amount: 0,
        }))
    }
}

impl UserStore {
    pub fn new(_: Arc<Client>, _: &Context<Self>) -> Self {
        let (_current_user_tx, current_user) = watch::channel_with(None);
        Self {
            users: HashMap::default(),
            participant_indices: HashMap::default(),
            current_user,
        }
    }

    pub fn clear_cache(&mut self) {}

    pub fn contacts(&self) -> &[Arc<Contact>] {
        &[]
    }

    pub fn has_contact(&self, _: &Arc<User>) -> bool {
        false
    }

    pub fn incoming_contact_requests(&self) -> &[Arc<User>] {
        &[]
    }

    pub fn outgoing_contact_requests(&self) -> &[Arc<User>] {
        &[]
    }

    pub fn is_contact_request_pending(&self, _: &User) -> bool {
        false
    }

    pub fn contact_request_status(&self, _: &User) -> ContactRequestStatus {
        ContactRequestStatus::None
    }

    pub fn request_contact(&mut self, _: Arc<User>, _: &mut Context<Self>) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn remove_contact(&mut self, _: u64, _: &mut Context<Self>) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn has_incoming_contact_request(&self, _: u64) -> bool {
        false
    }

    pub fn respond_to_contact_request(
        &mut self,
        _: u64,
        _: bool,
        _: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn dismiss_contact_request(&mut self, _: u64, _: &mut Context<Self>) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn clear_contacts(&self) -> impl std::future::Future<Output = ()> + use<> {
        std::future::ready(())
    }

    pub fn contact_updates_done(&self) -> impl std::future::Future<Output = ()> + use<> {
        std::future::ready(())
    }

    pub fn get_users(
        &mut self,
        _: Vec<u64>,
        _: &mut Context<Self>,
    ) -> Task<Result<Vec<Arc<User>>>> {
        Task::ready(Ok(Vec::new()))
    }

    pub fn fuzzy_search_users(
        &self,
        _: String,
        _: &mut Context<Self>,
    ) -> Task<Result<Vec<Arc<User>>>> {
        Task::ready(Ok(Vec::new()))
    }

    pub fn get_cached_user(&self, user_id: u64) -> Option<Arc<User>> {
        self.users.get(&user_id).cloned()
    }

    pub fn get_user_optimistic(&self, user_id: u64, _: &Context<Self>) -> Option<Arc<User>> {
        self.get_cached_user(user_id)
    }

    pub fn get_user(&self, user_id: u64, _: &Context<Self>) -> Task<Result<Arc<User>>> {
        let user = self.get_cached_user(user_id).unwrap_or_else(|| {
            Arc::new(User {
                id: user_id,
                github_login: SharedString::new(format!("user-{user_id}")),
                avatar_uri: SharedUri::from(""),
                name: None,
            })
        });
        Task::ready(Ok(user))
    }

    pub fn cached_user_by_github_login(&self, _: &str) -> Option<Arc<User>> {
        None
    }

    pub fn current_user(&self) -> Option<Arc<User>> {
        self.current_user.borrow().clone()
    }

    pub fn current_organization(&self) -> Option<Arc<Organization>> {
        None
    }

    pub fn set_current_organization(
        &mut self,
        _: Option<OrganizationId>,
        _: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    pub fn organizations(&self) -> &Vec<Arc<Organization>> {
        static EMPTY: std::sync::LazyLock<Vec<Arc<Organization>>> =
            std::sync::LazyLock::new(Vec::new);
        &EMPTY
    }

    pub fn plan_for_organization(&self, _: &OrganizationId) -> Option<Plan> {
        None
    }

    pub fn current_organization_configuration(&self) -> Option<&OrganizationConfiguration> {
        None
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_current_organization_configuration_for_test(
        &mut self,
        _: OrganizationConfiguration,
    ) {
    }

    pub fn plan(&self) -> Option<Plan> {
        None
    }

    pub fn subscription_period(&self) -> Option<()> {
        None
    }

    pub fn trial_started_at(&self) -> Option<()> {
        None
    }

    pub fn account_too_young(&self) -> bool {
        false
    }

    pub fn has_overdue_invoices(&self) -> bool {
        false
    }

    pub fn edit_prediction_usage(&self) -> Option<EditPredictionUsage> {
        None
    }

    pub fn update_edit_prediction_usage(&mut self, _: EditPredictionUsage) {}

    pub fn clear_organizations(&mut self) {}

    pub fn clear_plan_and_usage(&mut self) {}

    pub fn watch_current_user(&self) -> watch::Receiver<Option<Arc<User>>> {
        self.current_user.clone()
    }

    pub fn insert(&mut self, users: Vec<proto::User>) -> Vec<Arc<User>> {
        users
            .into_iter()
            .map(|message| {
                let user = Arc::new(User {
                    id: message.id,
                    github_login: message.github_login.into(),
                    avatar_uri: message.avatar_url.into(),
                    name: message.name,
                });
                self.users.insert(user.id, user.clone());
                user
            })
            .collect()
    }

    pub fn set_participant_indices(
        &mut self,
        participant_indices: impl IntoIterator<Item = (u64, ParticipantIndex)>,
        _: &mut Context<Self>,
    ) {
        self.participant_indices = participant_indices.into_iter().collect();
    }

    pub fn participant_indices(&self) -> &HashMap<u64, ParticipantIndex> {
        &self.participant_indices
    }

    pub fn participant_names(
        &self,
        participant_ids: impl IntoIterator<Item = u64>,
        _: &App,
    ) -> HashMap<u64, SharedString> {
        participant_ids
            .into_iter()
            .filter_map(|id| {
                self.get_cached_user(id)
                    .map(|user| (id, user.github_login.clone()))
            })
            .collect()
    }

    pub async fn handle_update_contacts(
        _: gpui::Entity<Self>,
        _: TypedEnvelope<proto::UpdateContacts>,
        _: gpui::AsyncApp,
    ) -> Result<()> {
        Ok(())
    }

    pub async fn handle_show_contacts(
        _: gpui::Entity<Self>,
        _: TypedEnvelope<proto::ShowContacts>,
        _: gpui::AsyncApp,
    ) -> Result<()> {
        Ok(())
    }
}
