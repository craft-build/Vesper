# 10 — Account Profile & Settings

## Goal

Turn `SettingsScreen` (tabbed shell in `screens/settings_screen.rs`) from
mock-shaped UI into the real account console: edit display name + avatar,
manage login sessions/devices (rename, verify, delete), notification
preferences, and full logout (incl. token invalidation).

## Deliverable / how to test

1. Profile tab: change display name → reflected instantly in Vesper shell
   and in Element's view of you; upload a new avatar → appears across Vesper
   (nav, message rows) via the media pipeline.
2. Devices tab: list matches `/devices` (cross-check in Element settings);
   rename a device → visible in Element; delete a throwaway session → that
   session is kicked (password re-auth via UIAA if required — see notes).
3. Notifications tab: existing toggles map to real push rules (global
   mute / per-rule toggles at minimum: mentions, DMs), persisted on the
   server — flip in Vesper, see effect.
4. Log out: invalidates the access token (verify: the token in session.json
   is dead — Element or curl against `_matrix/client/v3/whoami` with it
   returns 401) and wipes local stores.
5. Appearance/app prefs (theme, density — whatever the mock exposes) persist
   per-device (local file) and don't regress on restart.

## Context

- `SettingsTab` enum + `TABS` in `settings_screen.rs:14-44` define the
  surface; mock data feeds it today.
- Trait has `devices`, `verify_device`, `verify_user` (02/08); add
  profile/session/rule/preferences methods here.
- SDK surfaces: `client.account()` (`get_profile`, `set_display_name`,
  `upload_avatar`/`set_avatar_url`), `client.devices()`-family
  (`delete_devices` requires UIAA `AuthData`), `client.notification_settings()`
  (high-level push-rule API, 0.18 has `NotificationSettings`), and for
  device-local prefs just a JSON file in the data dir. Global account-data
  prefs (`client.account().set_account_data`) for anything that should roam.

## Design decisions

- **UIAA handling** (deleting devices, maybe more): implement a small
  password re-auth helper completing `m.login.password` stages
  (`uiaa` via `AuthData::Password(Password::new(identifier, pass))` with the
  `session` echo). Prompt UX: reuse the verify-dialog shell style for a
  "confirm with password" modal.
- **Device rename**: `rename_device(device_id, name)` — no UIAA needed.
- **Delete own device**: block deleting the *current* device in UI with a
  tooltip (use Log out instead).
- **Notifications tab scope**: read `notification_settings().get_...()` +
  set-rule-enabled calls for: master mute, contains-display-name, DM rooms,
  encrypted rooms. Mapping rules → friendly toggles lives in the client crate
  (table of rule ids → toggle), unit-tested.
- **Prefs versioning**: local prefs file `prefs.json` versioned
  (`{ version: 1, ... }`), tolerant serde defaults so future fields don't
  break old installs.
- All trait additions get MockClient implementations backed by the mock's
  in-memory state so settings UI remains demoable offline.

## Implementation steps

1. Trait: `profile() / set_display_name / set_avatar (upload via 07 pipeline)`,
   `rename_device / delete_devices(password)`,
   `notification_rules() -> Vec<NotifToggle> / set_notification_toggle(id, bool)`,
   `prefs() / set_prefs(...)`, `logout(invalidate: true)`.
2. Client impls incl. UIAA password-stage helper; wire logout to
   `matrix_auth().logout()` + store wipe (complete the tentative version from
   02).
3. SettingsScreen tabs wired one by one (profile fields with save states,
   devices table, toggles), keeping the existing visual system.
4. Notification toggle from 06's hard-coded `on` now reads prefs.
5. Unit tests: rule↔toggle mapping table, prefs serde round-trip.

## Acceptance criteria

- [ ] Name + avatar edits propagate to other clients.
- [ ] Device list/rename/delete truthful; current-device delete blocked.
- [ ] Notification toggles persist server-side and affect real delivery on a
      second account's messages (mentions fire, muted room doesn't).
- [ ] Logout invalidates the token server-side and wipes local data dir.
- [ ] Local prefs survive restart; unknown future fields don't crash.
- [ ] Mock mode settings flows still work for design iteration.

## AI implementation prompt

> Make SettingsScreen fully functional per docs/00 and docs/10. Extend
> VesperClient with profile get/set (display name, avatar upload through the
> checkpoint-07 media upload path), devices rename/delete with a reusable
> UIAA m.login.password helper (password-confirm modal reusing the verify
> dialog shell; block deleting the current device), notification toggles over
> client.notification_settings() with a tested rule-id→toggle mapping table
> (master mute, mentions, DMs, encrypted), versioned local prefs.json with
> serde-default tolerance, tools for roaming prefs via account data where
> sensible, and a proper logout that invalidates the token and wipes the
> local store. Wire every tab in screens/settings_screen.rs keeping the
> existing design system, give MockClient complete fake implementations, and
> verify against Element plus a second test account.

## Implemented / Deviations (retrospective footer)

**Implemented**: display-name editing, avatar upload, theme + local
prefs (typing/receipt opt-outs), device/session list with rename +
password-verified deletion (UIAA), server push-rule toggles, logout.

**Deviations**:
- The **media cache size + Clear row** the plan promised from "checkpoint
  10 settings" landed in checkpoint 11 §C with the cache itself.
- Notification-toggle failures surface as toasts with truth-refetch
  (checkpoint 11 §A) rather than an inline error line.
- A "Copy diagnostics" row (STORAGE/SUPPORT sections) was added in
  checkpoint 11 §D — redacted log tail + app facts, no message content.
