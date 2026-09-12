# Scryer Architecture

## Purpose

This document is the durable architectural contract for Scryer. It describes
system boundaries, ownership, and invariants. It is not a map of the current
repository, dependency graph, framework stack, or deployment scripts.

When code and this contract diverge, resolve the mismatch deliberately. Change
this document only when the architecture itself changes, not whenever an
implementation is reorganized.

## Product Model

Scryer is a self-hosted, single-node media management system for movies,
series, and anime. It discovers and evaluates releases, coordinates
acquisition, imports completed downloads, maintains organized libraries, and
projects that state to operators and integrations.

Scryer is intentionally:

- backend-authoritative
- relationally persisted
- event-driven internally
- GraphQL-primary at its application interface
- extensible through constrained plugins
- designed for one reliable node rather than distributed coordination

## Architectural Principles

### 1. The Backend Owns Product Truth

Authoritative product state and policy live in the backend. This includes
library identity, monitoring, acquisition, imports, quality decisions,
settings, permissions, event history, and plugin-mediated changes.

Media files, artwork, backups, and other large payloads may live on the
filesystem. Protected credentials may live in an operating-system secret store
or encrypted backend-managed storage. These remain explicit backend-owned
resources rather than alternate sources of product policy.

The frontend and plugins may request, project, and present changes. They do not
become independent authorities.

### 2. Dependencies Point Toward Policy

Scryer follows conceptual layers:

- domain types and invariants
- application policy, workflows, and ports
- infrastructure adapters for persistence and external systems
- transport adapters for public interfaces
- process composition and startup

Dependencies point toward domain and application policy. Inner layers do not
depend on transport, persistence engines, runtime composition, or user
interface details. Adapters translate at boundaries; they do not acquire
business policy merely because they can reach an external system.

The physical organization may evolve. The dependency direction may not drift
with it.

### 3. One Durable Domain Event Vocabulary

Every meaningful durable state transition must be representable as a domain
event. Domain events are product facts, not payloads designed for one consumer.

The same event vocabulary drives activity, history, subscriptions,
notifications, and background reactions. Scryer does not create shadow event
systems for individual features.

Events require stable identity, ordering, actor context, and enough domain
context for authorized consumers to understand what happened. Consumers must
be idempotent because delivery, retries, and restart recovery may repeat work.

### 4. Durable State Precedes Live Fanout

State changes and their durable event intent are established before live
wake-up or fanout is considered successful. When they form one persistence
unit, they should commit atomically.

Transient channels are wake mechanisms, not records of truth. Subscribers and
background processors recover by reading durable state from a cursor or
checkpoint. A restart must not erase the explanation of what happened.

### 5. Public Interfaces Express Intent

GraphQL is the primary application query, mutation, and subscription
interface. Its operations describe user and integration intent rather than
tables, storage records, or internal orchestration.

Narrow HTTP routes are appropriate for transport-specific concerns such as
OAuth, health and metrics, binary transfer, images, and the web application
shell. These routes remain adapters over the same authorization and product
rules; they are not a second application layer.

Boundary models are explicit and typed. Storage encodings and internal
implementation details do not become public contracts by accident.

### 6. Persistence Engines Preserve One Product Model

Every first-class relational datastore engine implements the same logical
product behavior. Engine-specific SQL, connection handling, retries, and
maintenance remain behind persistence boundaries.

Durable datastore changes require equivalent treatment across supported
engines unless the behavior is explicitly engine-local. Multi-step mutations
use transactions. Dynamic SQL remains parameterized and bounded.

Historical migrations are immutable after release. New behavior or correction
uses a new migration.

Backup bundles are logical, validated, and portable across supported engines.
Restore is a product workflow with explicit compatibility, integrity, secret,
and restart behavior.

Cross-version restores will never be supported. A backup must be restored into
the same Scryer version that created it, including when changing datastore
engines. Restore compatibility must not translate historical schemas or accept
another version's table set. Rejecting a cross-version backup is intentional
product behavior, not a defect or a missing compatibility feature.

Recovery uses the version that created the backup. Any subsequent upgrade is a
separate workflow through the normal database migrations; it is not part of
restore. Reviews and automated agents must preserve this boundary.

Scryer's single-node design does not require Redis, external queues, message
brokers, or a distributed control plane.

### 7. Media Workflows Keep Distinct Ownership

Related media workflows must not collapse into one generic processing layer:

- acquisition decides what to search for and obtain
- external integration tracks work owned by other systems
- import decides what completed work means and how it enters a library
- library management owns the durable organized result
- metadata and artwork own identity enrichment and presentation assets
- quality, parsing, scoring, and rules own release evaluation

Movies, series, and anime are explicit facets of the domain. Facet-specific
behavior belongs in deliberate policy, not scattered string checks and
one-off branches.

### 8. Concurrency and Memory Are Bounded

Every queue, channel, cache, buffer, retry loop, scan, and batch has an explicit
bound and lifecycle. Background work supports cancellation and orderly
shutdown. Retry behavior has limits, backoff, and an observable terminal state.

Large datasets are filtered and paginated near their source. The system does
not rely on assumptions that a library, event stream, or result set will remain
small.

### 9. Mutable Runtime State Has One Owner

Runtime channels, trackers, wake handles, caches, and coordination guards have
clear ownership and construction. They do not become hidden product state.

Durable dependencies and mutable runtime coordination remain conceptually
separate. Process-wide state is acceptable only when its lifecycle is explicit,
its scope is narrow, and durable correctness does not depend on its survival.

### 10. Internal Boundaries Stay Typed

Stable statuses, event types, permissions, settings identifiers, media facets,
and policy decisions use typed representations internally.

Persistence and public interfaces use explicit mappings. Serialized tokens are
durable contracts and require migration or compatibility treatment when they
change. Free-form strings and generic JSON are reserved for genuinely
open-ended data, diagnostics, or compatibility boundaries.

### 11. The Frontend Is a Projection Client

The frontend owns presentation, interaction, navigation, and local ephemeral
state. The backend owns durable state and policy.

Frontend code consumes public interfaces and does not share backend
implementation types. It may perform local projection and optimistic
interaction only when authoritative backend state can reconcile the result.
User-visible behavior remains accessible, localizable, and recoverable after a
refresh.

### 12. Plugins Are Constrained Guests

Plugins extend explicit host capabilities. They are sandboxed, versioned,
observable, and subject to host authorization, resource, and compatibility
rules.

Plugins do not access core storage directly, bypass policy, or establish hidden
side channels. New plugin power is introduced through a reviewed host
capability or hook with a stable contract.

### 13. Security and Side Effects Are Product Behavior

Every operation answers who may perform it, on whose behalf it runs, which
resources it may reach, what secrets it may access, and how it behaves after
failure or restart.

Filesystem operations stay within validated roots. Destructive actions require
explicit intent and verified preconditions. Secrets do not enter logs, metrics,
public errors, or unencrypted persistence.

Host-owned remote HTTP uses a shared transport policy for trust, rate limits,
timeouts, retries, and observability. Safe redirects are normal upstream
service behavior for indexers, metadata and image providers, download clients,
and similar integrations. The shared transport may follow them when hop count
is bounded and its normal timeout, retry, per-hop rate-limit, and observability
rules remain in effect. Redirect rejection is an explicit protocol exception,
not the default security posture. Callers remain responsible for validating
the initial endpoint and the response they consume.

A specialized helper protocol may own different transport semantics only when
the exception is explicit, bounded, tested, and observable.

### 14. Operations Are Part of the Product

Startup, migrations, key bootstrap, backup and restore, health reporting,
shutdown, and upgrades are first-class workflows. They fail clearly, preserve
recoverability, and expose enough state for an operator to understand what the
system is doing.

Operational shortcuts must not create a second source of truth or bypass the
same invariants enforced during normal runtime.

### 15. Use the Least Necessary Code

Prefer extending a coherent path over creating a parallel subsystem. Add an
abstraction only when it removes real complexity, enforces a meaningful
boundary, or eliminates material duplication.

Deleting or consolidating code while preserving behavior is a successful
architectural outcome. Compactness is not an excuse for obscure logic; the goal
is less unnecessary surface area.

## Canonical Change Flow

A state-changing feature should follow this recognizable flow:

1. A semantic request enters through a public or internal boundary.
2. Actor, authorization, and context are established.
3. Application policy validates the request and invariants.
4. Authoritative state and durable event intent are persisted.
5. The transaction commits before live wake-up or downstream dispatch.
6. Consumers re-read durable state and project authorized results.
7. Retry, cancellation, restart, and partial failure remain explainable.

Not every feature uses every stage, but deviations require an explicit reason.

## Verification Contract

Tests follow the boundary being protected. Changes should prove, where
relevant:

- domain and policy behavior
- datastore-engine parity
- public interface contracts and authorization
- event durability, ordering, and projection
- plugin capability and compatibility boundaries
- cancellation, retry, restart, and partial-failure behavior
- bounded resource use for large inputs
- integrity of imports, backups, restores, and destructive filesystem actions

Performance claims require equivalent work, verified outputs, reproducible
workloads, and enough measurements to support the conclusion.

## Red Flags

Stop and reconsider when a change introduces:

- an inner layer depending on an adapter or transport
- durable state changes with no meaningful domain event
- live delivery treated as the source of truth
- business policy in the frontend, transport, persistence, or plugin runtime
- storage-shaped records or stringly state crossing internal boundaries
- behavior implemented for only one first-class datastore engine
- unbounded queues, caches, result sets, scans, or retries
- plugin or external integration paths that bypass host policy
- destructive work without verified scope and preconditions
- a new framework or subsystem for a hypothetical future requirement

## Change Rule

Architecture changes are intentional product decisions. Update this document in
the same change, explain the tradeoff, and update affected public contract
documentation. Routine refactors should continue to satisfy this contract
without rewriting it to mirror the latest repository shape.
