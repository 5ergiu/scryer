# Scryer 0.21.19 release notes

These changes are relative to v0.21.18.

## Highlights

- Maintenance rules and request rules are no longer experimental. They are available on every instance without turning on experimental features.
- Give each maintenance rule one **Disabled**, **Dry run**, or **Enabled** state, controlled together with two instance switches: **Maintenance** and **Pause all actions**. Request rules are enabled with a single checkbox.
- Ship a built-in maintenance rule, **Expired request leases**, that deletes requested titles seven days after their request lease expires unless they are kept. Read the upgrade notes below before upgrading an instance with **Maintenance** switched on.
- Keep a title with one click from maintenance candidates, the title page, or the Requests claims panel.
- Run scheduled jobs independently, so a long RSS sync no longer delays housekeeping, health checks, Prowlarr sync, or pending-release processing.

## Maintenance rules

- Each rule has a single state: **Disabled**, **Dry run**, or **Enabled**. **Dry run** evaluates the rule and records what it would do without acting; **Enabled** applies the rule's actions. Editing an enabled rule returns it to **Dry run** and marks it as edited since it was last enabled.
- Two instance switches appear above every maintenance pane. **Maintenance** controls whether rules are evaluated and their results shown on the instance. **Pause all actions** stops every rule from acting, reversible and destructive alike, while evaluation continues. The separate gates pane is gone; existing links to it open the rule list.
- Enabling a rule that deletes files shows the full list of current candidates in the confirmation.
- **Keep** a title from its candidate row, from the title page, or from the Requests claims panel. Maintenance rules do not delete a kept title or its files while the keep is in place, and the title shows as **Kept** with a control to release the keep. Keeping a title that is already kept returns the existing keep. Keeping a title requires title management permission on its library.
- Exclusions are now listed under the candidates they were made from.
- The rule editor now lists name, template, matcher, action sequence, grace period, and libraries in that order. Rule scope and storage root sit under **Advanced**, which opens automatically when either has a non-default value.
- Rules can use list facts: which enabled lists hold a title, public and personal list counts, first and last seen dates, days on a list, best rank, on-leave and departure-handled status, and membership states. They can also read how a title was requested and the name of the public list it came from. Personal list names are never exposed. A fact that cannot be read is reported as unknown rather than treated as absent.
- Add two templates: **Left every list**, which unmonitors titles every list has dropped once the departures are handled and more than 30 days old, and **Added by a list and never watched**.

## Request rules

- A request rule is enabled with one checkbox. Requests are decided by the enabled rules that cover the request's library; there are no evaluation modes and no instance-wide gate.
- Rules that read requester fields still require the permissions-management right, and the rules page marks those rules.
- Rules can read the request's origin and, for requests that came from a list, the list's subscription, scope, mode, provider, owner policy, rank, and public name. If the list cannot be read, that section is omitted and the request is still evaluated.
- Add two templates: hold requests from lists that use the Approval policy, and deny personal-list requests above 1080p.

## Expired request leases

- Scryer now ships a built-in maintenance rule, **Expired request leases**. Seven days after a requested title's request lease expires, the rule deletes the title and its files unless the title has a Keep (a forever request or an operator Keep). It starts **Enabled** on every instance but, like every maintenance rule, only acts while **Maintenance** is switched on and actions are not paused. On upgrade with **Maintenance** on, titles whose leases had already expired become candidates on the first evaluation pass and are deleted seven days later; to prevent that, Keep those titles, set the rule to **Dry run** or **Disabled**, or switch on **Pause all actions**. The rule cannot be renamed or deleted and its matcher and action cannot be changed; its grace period, libraries and state can be edited, and changing the grace period or libraries sets it to **Dry run** until it is enabled again.
- The rule is marked as built in on the maintenance rules list, and the request rules page includes a control for its state and grace period.

## Parsing and imports

- Recognize episode revision suffixes such as `S07E12v2` and `7x12v2` while keeping the season and episode, and record the revision so the release is treated as a proper upload.
- Leave revision-like text inside a known title as part of the title, and keep resolutions with a revision suffix from being read as episode numbers.
- During manual import, use the catalog's own numbering when a community anime season index collides with an official season and every file falls inside the grabbed episodes.
- Focus the search field when the manual import episode picker opens, and keep long episode selections from widening the import table.

## Scheduling and jobs

- Run every scheduled job in its own task. A job that is still running when its next run is due skips that run rather than starting a second copy, and jobs running past their expected time are reported in the logs and metrics without being stopped.
- Clear a finished job run from the Jobs view even when its final status cannot be written, so the job no longer appears stuck and can be started again. The failed write is still logged and reported.

## Compatibility and upgrade notes

- Existing rules are migrated on a best-effort basis. Maintenance rules that were in shadow mode become **Dry run** and need to be enabled again. Request rules that were in shadow mode become **Disabled**. The previous maintenance gate settings and the request-rules instance gate are removed and must be set again through the new switches: **Maintenance** and **Pause all actions** for maintenance rules, and each request rule's **Enabled** checkbox for request rules.
- GraphQL clients must move from the removed `setMaintenanceRuleMode`, `setMaintenanceRuleArming`, `setRequestRuleMode`, and `setRequestRuleInstanceGates` mutations, the `requestRuleInstanceGates` field, the `SHADOW` enum value, the `includeShadow` argument, `evaluationMode` and `effectArming` on maintenance rule sets, and `evaluationMode` on request rule sets to `setMaintenanceRuleState`, `setMaintenanceInstanceGates`, and `setRequestRuleEnabled`.
- Add Prometheus metrics for skipped overlapping scheduled runs, long-running scheduled jobs, and failed job-run status writes.
