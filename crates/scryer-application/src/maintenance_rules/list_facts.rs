//! List membership facts for maintenance rules.
//!
//! Lists never remove a title. An operator who wants "remove what no list
//! wants any more" writes it as a maintenance rule, and these facts are what
//! that rule reads. They are loaded once per batch of titles and laid over
//! the fact snapshot the builder produced.
//!
//! Personal list names never appear here: a personal list still counts for
//! `lists_on_enabled_list` and `lists_left_all`, but only public lists are
//! named.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use scryer_domain::{ListMembership, ListScope, ListSubscription, MediaRequestOrigin, Title};
use scryer_rules::maintenance::{MaintenanceInput, Observation};

use super::facts::unknown_reason;
use crate::lists::ListSubscriptionQuery;
use crate::{AppResult, AppUseCase};

/// What the list store says about one title.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TitleListFacts {
    pub added_by_list: bool,
    pub on_enabled_list: bool,
    pub names: Vec<String>,
    pub left_all: bool,
    pub left_at: Option<DateTime<Utc>>,
    pub last_list_name: Option<String>,
    pub enabled_ids: Vec<String>,
    pub public_count: i64,
    pub personal_count: i64,
    pub first_seen_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub best_rank: Option<i64>,
    pub on_leave: Vec<String>,
    /// `None` when no list has dropped the title.
    pub left_handled: Option<bool>,
    pub membership_states: Vec<String>,
}

/// List facts for a batch, keyed by title id. A title with no entry is on no
/// list and never was.
pub(crate) type ListFactsByTitle = HashMap<String, TitleListFacts>;

/// Where the request that created a title came from. `list_name` is set only
/// for a public list the list store still holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TitleRequestOrigin {
    pub origin: MediaRequestOrigin,
    pub list_name: Option<String>,
}

/// Everything the list and request stores said about a batch. Each is `None`
/// when its store could not be read, so only the facts it feeds read unknown.
#[derive(Clone, Debug)]
pub(crate) struct MaintenanceListFacts {
    pub lists: Option<ListFactsByTitle>,
    pub origins: Option<HashMap<String, TitleRequestOrigin>>,
}

impl AppUseCase {
    /// One read each of the requests, memberships and subscriptions for a
    /// batch. A failed read leaves the facts it feeds unknown, so the rule is
    /// held rather than the pass failing for rules that never mention a list.
    pub(crate) async fn maintenance_list_facts_or_unknown(
        &self,
        titles: &[Title],
    ) -> MaintenanceListFacts {
        let title_ids: Vec<String> = titles.iter().map(|title| title.id.clone()).collect();
        if title_ids.is_empty() {
            return MaintenanceListFacts {
                lists: Some(HashMap::new()),
                origins: Some(HashMap::new()),
            };
        }
        let origins = match self
            .services
            .catalog
            .media_requests
            .request_origins_by_title_ids(&title_ids)
            .await
        {
            Ok(origins) => Some(origins),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "could not read request origins; request origin facts are unknown for this batch"
                );
                None
            }
        };
        let needs_public_names = origins.as_ref().is_some_and(|origins| {
            origins
                .values()
                .any(|origin| matches!(origin, MediaRequestOrigin::PublicList { .. }))
        });
        let store = match self
            .maintenance_list_store_rows(&title_ids, needs_public_names)
            .await
        {
            Ok(store) => Some(store),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "could not read list memberships; list facts are unknown for this batch"
                );
                None
            }
        };
        MaintenanceListFacts {
            origins: origins.map(|origins| {
                let subscriptions = store
                    .as_ref()
                    .map_or(&[][..], |(_, subscriptions)| subscriptions);
                request_origins_with_names(origins, subscriptions)
            }),
            lists: store.map(|(rows, subscriptions)| list_facts_from_rows(&rows, &subscriptions)),
        }
    }

    /// The memberships of a batch, and the subscriptions when there are any
    /// memberships or a request needs a public list's name.
    async fn maintenance_list_store_rows(
        &self,
        title_ids: &[String],
        needs_subscriptions: bool,
    ) -> AppResult<(Vec<ListMembership>, Vec<ListSubscription>)> {
        let lists = &self.services.lists;
        let rows = lists.memberships.list_by_titles(title_ids).await?;
        if rows.is_empty() && !needs_subscriptions {
            return Ok((rows, Vec::new()));
        }
        let subscriptions = lists
            .subscriptions
            .list(ListSubscriptionQuery::default())
            .await?;
        Ok((rows, subscriptions))
    }
}

pub(crate) fn list_facts_from_rows(
    rows: &[ListMembership],
    subscriptions: &[ListSubscription],
) -> ListFactsByTitle {
    let subscriptions: HashMap<&str, &ListSubscription> = subscriptions
        .iter()
        .map(|subscription| (subscription.id.as_str(), subscription))
        .collect();
    let mut by_title: HashMap<&str, Vec<&ListMembership>> = HashMap::new();
    for row in rows {
        if let Some(title_id) = row.title_id.as_deref() {
            by_title.entry(title_id).or_default().push(row);
        }
    }

    by_title
        .into_iter()
        .map(|(title_id, rows)| {
            let present: Vec<&ListMembership> = rows
                .iter()
                .copied()
                .filter(|row| row.left_at.is_none())
                .collect();
            let mut names = BTreeSet::new();
            let mut enabled_ids = BTreeSet::new();
            let mut on_leave = BTreeSet::new();
            let mut public_ids = BTreeSet::new();
            let mut personal_ids = BTreeSet::new();
            for row in &present {
                let Some(subscription) = subscriptions.get(row.subscription_id.as_str()) else {
                    continue;
                };
                match subscription.scope {
                    ListScope::Public => public_ids.insert(subscription.id.as_str()),
                    ListScope::Personal => personal_ids.insert(subscription.id.as_str()),
                };
                if !subscription.enabled {
                    continue;
                }
                enabled_ids.insert(subscription.id.clone());
                on_leave.insert(subscription.on_leave.as_str().to_string());
                if subscription.scope == ListScope::Public {
                    names.insert(subscription.name.clone());
                }
            }
            let left_all = rows.iter().all(|row| row.left_at.is_some());
            let left_at = left_all
                .then(|| rows.iter().filter_map(|row| row.left_at).max())
                .flatten();
            let last_list_name = rows
                .iter()
                .filter_map(|row| {
                    let subscription = subscriptions.get(row.subscription_id.as_str())?;
                    (subscription.scope == ListScope::Public)
                        .then_some((row.left_at?, subscription.name.clone()))
                })
                .max_by(|left, right| left.0.cmp(&right.0).then(right.1.cmp(&left.1)))
                .map(|(_, name)| name);
            let departed: Vec<bool> = rows
                .iter()
                .filter(|row| row.left_at.is_some())
                .map(|row| row.left_handled)
                .collect();
            (
                title_id.to_string(),
                TitleListFacts {
                    added_by_list: rows.iter().any(|row| row.added_by_list),
                    on_enabled_list: !enabled_ids.is_empty(),
                    names: names.into_iter().collect(),
                    left_all,
                    left_at,
                    last_list_name,
                    enabled_ids: enabled_ids.into_iter().collect(),
                    public_count: public_ids.len() as i64,
                    personal_count: personal_ids.len() as i64,
                    first_seen_at: present.iter().map(|row| row.first_seen_at).min(),
                    last_seen_at: present.iter().map(|row| row.last_seen_at).max(),
                    best_rank: present.iter().filter_map(|row| row.rank).min(),
                    on_leave: on_leave.into_iter().collect(),
                    left_handled: (!departed.is_empty())
                        .then(|| departed.iter().all(|handled| *handled)),
                    membership_states: present
                        .iter()
                        .map(|row| row.state.as_str().to_string())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                },
            )
        })
        .collect()
}

/// Name each public-list origin from the subscriptions; every other origin,
/// and a public list the store no longer holds, stays unnamed.
pub(crate) fn request_origins_with_names(
    origins: HashMap<String, MediaRequestOrigin>,
    subscriptions: &[ListSubscription],
) -> HashMap<String, TitleRequestOrigin> {
    origins
        .into_iter()
        .map(|(title_id, origin)| {
            let list_name = match &origin {
                MediaRequestOrigin::PublicList { subscription_id } => subscriptions
                    .iter()
                    .find(|subscription| {
                        subscription.id == *subscription_id
                            && subscription.scope == ListScope::Public
                    })
                    .map(|subscription| subscription.name.clone()),
                _ => None,
            };
            (title_id, TitleRequestOrigin { origin, list_name })
        })
        .collect()
}

/// Lay the batch's list and request-origin facts over one subject's snapshot.
/// A store that could not be read leaves every fact it feeds unknown.
pub(crate) fn apply_list_facts(
    input: &mut MaintenanceInput,
    loaded: &MaintenanceListFacts,
    title_id: &str,
) {
    let evaluation_time = input.evaluation_time;
    let facts = &mut input.facts;
    let origin = loaded
        .origins
        .as_ref()
        .map(|origins| origins.get(title_id).cloned());
    (facts.request_origin, facts.request_list_name) = match origin {
        None => {
            let reason = unknown_reason::REQUEST_STORE_UNREADABLE;
            (Observation::unknown(reason), Observation::unknown(reason))
        }
        Some(None) => {
            let reason = unknown_reason::TITLE_NOT_REQUESTED;
            (
                Observation::absent_because(reason),
                Observation::absent_because(reason),
            )
        }
        Some(Some(TitleRequestOrigin { origin, list_name })) => (
            Observation::known(origin.as_str().to_string()),
            match (list_name, &origin, &loaded.lists) {
                (Some(name), _, _) => Observation::known(name),
                (None, MediaRequestOrigin::PublicList { .. }, None) => {
                    Observation::unknown(unknown_reason::LIST_STORE_UNREADABLE)
                }
                (None, _, _) => Observation::absent_because(unknown_reason::NO_PUBLIC_REQUEST_LIST),
            },
        ),
    };

    let Some(lists) = &loaded.lists else {
        let reason = unknown_reason::LIST_STORE_UNREADABLE;
        facts.lists_added_by_list = Observation::unknown(reason);
        facts.lists_on_enabled_list = Observation::unknown(reason);
        facts.lists_names = Observation::unknown(reason);
        facts.lists_left_all = Observation::unknown(reason);
        facts.lists_left_at = Observation::unknown(reason);
        facts.lists_last_list_name = Observation::unknown(reason);
        facts.lists_enabled_ids = Observation::unknown(reason);
        facts.lists_public_count = Observation::unknown(reason);
        facts.lists_personal_count = Observation::unknown(reason);
        facts.lists_first_seen_at = Observation::unknown(reason);
        facts.lists_last_seen_at = Observation::unknown(reason);
        facts.lists_days_on_list = Observation::unknown(reason);
        facts.lists_best_rank = Observation::unknown(reason);
        facts.lists_on_leave = Observation::unknown(reason);
        facts.lists_left_handled = Observation::unknown(reason);
        facts.lists_membership_states = Observation::unknown(reason);
        return;
    };
    let title = lists.get(title_id).cloned().unwrap_or_default();
    fn not_on_a_list<T: serde::Serialize>() -> Observation<T> {
        Observation::absent_because(unknown_reason::NOT_ON_A_LIST)
    }
    facts.lists_added_by_list = Observation::known(title.added_by_list);
    facts.lists_on_enabled_list = Observation::known(title.on_enabled_list);
    facts.lists_names = Observation::known(title.names);
    facts.lists_left_all = Observation::known(title.left_all);
    facts.lists_left_at = title.left_at.map_or_else(
        || Observation::absent_because(unknown_reason::NOT_LEFT_EVERY_LIST),
        |left_at| Observation::known(left_at.to_rfc3339()),
    );
    facts.lists_last_list_name = title.last_list_name.map_or_else(
        || Observation::absent_because(unknown_reason::NO_PUBLIC_LIST_LEFT),
        Observation::known,
    );
    facts.lists_enabled_ids = Observation::known(title.enabled_ids);
    facts.lists_public_count = Observation::known(title.public_count);
    facts.lists_personal_count = Observation::known(title.personal_count);
    facts.lists_first_seen_at = title
        .first_seen_at
        .map_or_else(not_on_a_list, |at| Observation::known(at.to_rfc3339()));
    facts.lists_last_seen_at = title
        .last_seen_at
        .map_or_else(not_on_a_list, |at| Observation::known(at.to_rfc3339()));
    facts.lists_days_on_list = title.first_seen_at.map_or_else(not_on_a_list, |at| {
        Observation::known((evaluation_time - at).num_days().max(0))
    });
    facts.lists_best_rank = title.best_rank.map_or_else(
        || Observation::absent_because(unknown_reason::NO_LIST_RANK),
        Observation::known,
    );
    facts.lists_on_leave = Observation::known(title.on_leave);
    facts.lists_left_handled = title.left_handled.map_or_else(
        || Observation::absent_because(unknown_reason::NO_LIST_LEFT),
        Observation::known,
    );
    facts.lists_membership_states = Observation::known(title.membership_states);
}

#[cfg(test)]
#[path = "list_facts_tests.rs"]
mod tests;
