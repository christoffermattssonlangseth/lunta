# Design: shared group libraries

Status: proposal (22 Nov 2026). Nothing here is built. It describes how a research group could share a
library in Lunta without a Lunta server or a Lunta account, what that needs from the core and the apps, and in
which order to build it.

The setting it is written for: a group of 5–20 people at Karolinska Institutet (say the Castelo-Branco lab),
on a mix of Macs, Windows PCs, iPhones and Android phones, who want one common collection of papers next to
each person's own library.

## 1. Use cases and requirements

**Use cases**

1. *The lab's papers.* Everyone sees the same papers, PDFs and reading views. A new student opens the group and
   has the lab's 600 key papers, with the seniors' highlights, on day one.
2. *Journal club.* One person adds this week's paper; everyone reads it, highlights and writes notes; at the
   meeting the presenter shows everyone's highlights, or makes an Outline / Summary from them
   (`core/src/outline.rs`, `core/src/outline/summary.rs` work on any set of highlights).
3. *A shared review or grant.* Folders for the sections ("Introduction", "Iron and microglia"), tags, and one
   set of citation keys, so a shared LaTeX `.bib` or Word document cites the same keys for everyone.
4. *Who added what.* "Added by Anna, 3 days ago"; a highlight shows its author's initials.
5. *Private reading.* I can read a group paper, mark it read, star it and keep my own notes without the
   group seeing them.
6. *Joining and leaving.* A new member joins in minutes; someone who leaves keeps their own library, and the
   group keeps the papers they added.

**Requirements**

- R1. No Lunta server and no Lunta account. The group's storage is something the group already has: Microsoft
  365 (OneDrive, SharePoint, Teams), iCloud Drive, Google Drive, a network share.
- R2. Works offline, like the personal library: every change is written locally and syncs later.
- R3. Mixed platforms. Phase 1 may leave out phones, but the data model must not.
- R4. The personal library is untouched: a group is a *second* library, never merged into the first.
- R5. Shared: papers (metadata), PDFs, reading views, full texts, highlights and notes, tags, folders,
  citation keys. Personal: read / starred / Must Read, reading positions, and (later) private highlights.
- R6. Roles: owner, editors, readers. Honest about what can be enforced without a server (section 5).
- R7. Licences: the group can choose not to share PDFs, or only open-access ones.
- R8. Scales to 20 people with 3–4 devices each, for years.

## 2. Where the shared library lives: options

Every option below keeps the existing layout (`docs/development.md`, *Library layout*): `papers/<id>/…`
written once, `ops/<device>.jsonl` written only by its device, replayed by everyone (`core/src/store.rs`,
`Store::replay`). That layout is what makes a *shared* folder workable at all: 20 people never write the same
file, so no sync service ever has to resolve a conflict. The options differ in who moves the files.

### (a) A shared Google Drive folder

Two ways to reach it:

- **Google Drive for desktop** (Mac, Windows): the owner shares a folder; each member adds it to My Drive
  ("Add shortcut to Drive") and it appears in their synced Drive folder. Lunta opens it as a folder. This
  works today in the Windows/Linux app, which already opens any library folder
  (`desktop/src-tauri/src/lib.rs`, `AppState::open_library`; `settings.rs`: "inside Google Drive, Dropbox,
  OneDrive, or anywhere local"). Not on phones, not on Linux (no official client).
- **The Drive API** (`core/src/drive/`, Android, iPhone/iPad in Google Drive mode): this is where `drive.file`
  bites.

**`drive.file` and files made by other people: it does not work.** `drive.file` grants access *per user, per
file*: the files the app created *for this user*, and the items this user picked in Google's picker. Google's
scope documentation ("Choose Google Drive API scopes") describes it as access to files "that you open with
an app or that the user shares with an app while using the Google Picker API". It says nothing about a file
that the same app created for another user, but developers who tried it report the same result: a file
created through the app by user A and then shared with user B gives B's token **404 Not Found**, and works at
once with the full `drive` scope. That agrees with what we already know from our own work
(`docs/development.md`, *Access: drive.file only*): picking a folder grants the folder but not the files
already in it, and files that another *app* (Drive for desktop) creates stay invisible. For a group that
means:

- Anna's Android phone uploads `ops/android-anna.jsonl` and `papers/<id>/paper.pdf` into the shared folder.
  Björn's phone, signed in with `drive.file`, does not see either: not in `files.list`, not in `changes.list`.
- Björn could pick each file in the picker, but new files appear every day (each paper is a new folder of
  new files). Not usable.
- Shared drives (Team Drives) change nothing: the grant is still per user per file.

So group sync through the Drive API needs the **full `drive` scope** (restricted: a yearly CASA security
assessment, and 100 users until verified), or at least `drive.readonly` (also restricted) for reading
others' files plus `drive.file` for writing one's own. The core still asks for the full scope today
(`NARROW_DRIVE_ACCESS = false` in `core/src/drive/auth.rs`), so a prototype could work now, but the plan is
to drop that scope, and building a group feature on it would commit Lunta to CASA every year. Also
unverified: whether `changes.list` reports changes inside a folder that is *shared with me* and not in My
Drive (the incremental sync depends on it); `docs/development.md` says a library shared by another account
"was never supported". **Conclusion: a shared Google Drive folder is fine through Drive for desktop on
Mac/Windows; through the API (phones) only with a restricted scope. Not the first choice.**

### (b) iCloud

- **iCloud Drive folder sharing** (macOS 12.3+, iOS 15.4+, iCloud for Windows 12+): the owner shares a folder
  in iCloud Drive with people by Apple ID; it appears in each participant's iCloud Drive, read-write or read
  only, and syncs like any iCloud folder. Lunta would open it with a security-scoped bookmark (the Mac app is
  sandboxed, `apple/project.yml` `com.apple.security.app-sandbox: true`; on iPhone/iPad
  `UIDocumentPickerViewController` for a folder). Pros: nothing to build on the storage side, works on iPhone
  and iPad, read-only participants are enforced by Apple. Cons: Apple IDs needed (Windows through iCloud for
  Windows, no Android, no Linux); the folder lives in iCloud Drive proper, *not* inside Lunta's container
  (`iCloud~com~cmlangseth~margins/Documents`), so `CloudOpsWatcher` (`apple/Shared/CloudOpsWatcher.swift`,
  `NSMetadataQueryUbiquitousDocumentsScope`) does not see it: watch it with an `NSMetadataQuery` scoped to
  the folder's URL, or poll `Library::refresh` (cheap: it only stats the logs, `Store::needs_replay`).
  Files may be evicted (dataless), so reads go through file coordination as in iCloud mode today. Must check:
  whether a folder *inside* the app's container can be shared (probably not offered; create group folders
  in iCloud Drive's root instead).
- **CloudKit sharing (`CKShare`)**: the right Apple tool for record-level sharing with permissions, but it means
  a second storage model (records, not files), Apple-only (CloudKit JS exists, nothing for Android), and the
  group's data in Apple's database for Lunta's container. Rejected: it duplicates the file layout that every
  other platform uses.

### (c) OneDrive / SharePoint / Teams, Dropbox, Box via their desktop clients

The group's folder is a OneDrive shared folder, a SharePoint document library or a Teams channel's "Files"
(all SharePoint underneath), synced by the OneDrive client ("Sync" or "Add shortcut to My files"). Lunta opens
it as a folder: the Windows app can do this today, the Mac app needs "open a library folder" (a picked folder
with a bookmark; the Mac only offers iCloud / Google Drive / this device today, `apple/Shared/StorageDecision.swift`).

- Pros: KI already has Microsoft 365, so this is storage the university approves, with its contracts, retention,
  versioning (files can be restored for 93 days from the recycle bin) and its own sharing and read-only
  permissions. No OAuth client of Lunta's involved, no app verification. Dropbox and Box work the same way
  for groups that use them.
- Cons: Mac and Windows only. Linux has no official OneDrive client (`abraunegg/onedrive` and rclone work).
  On phones, the OneDrive app's file provider does not keep a folder synced in the background, so Lunta
  would have to read and write through it on demand: possible on iOS through a bookmarked folder, unreliable
  for an append-only log; on Android the Storage Access Framework gives content URIs one file at a time.
  Phones need an API (Phase 2). Files On-Demand: logs and PDFs may be placeholders; reading them downloads
  them (macOS File Provider, Windows cloud files), which `Store::replay` already tolerates (a partly synced
  last line is skipped).

### (d) A small self-hosted relay

A Lunta relay (a process that stores logs and blobs and forwards changes) is against Lunta's principle: someone
must run it, it holds the group's data (making its operator a GDPR processor), and Lunta would have to support
it. Not recommended as a Lunta product.

There is a middle way that keeps the principle: **storage the group already has, spoken to by the core's
`RemoteStore` trait** (`core/src/drive/remote.rs`: `list`, `download`, `upload`, `delete`; `Syncer` does the
rest). A **WebDAV** backend is ~400 lines and reaches Nextcloud, ownCloud and SUNET Drive (the Nextcloud
service run by SUNET for Swedish universities, if KI uses it), with folder sharing between users built in and
app passwords instead of an OAuth app. A **Microsoft Graph** backend (`/drives/{id}/items/{id}/delta` is the
counterpart of Drive's changes API) reaches OneDrive and SharePoint from phones. Neither is a Lunta server.

### (e) A `.lunta` bundle: export and import

"Share Folder as Lunta Bundle…": a zip with the same layout (`papers/<id>/…` for the chosen papers, one log
`ops/bundle-<id>.jsonl` with their `AddPaper`, `Tag`, `CollectionUpsert`, `Member` and `AnnotationUpsert` ops,
each highlight's author's name). Importing adds the papers to a library (`Library::add_paper` already merges
by DOI) and the highlights to them. No sync, no permissions, nothing to keep in step; useful on its own
(sending a reading list to a collaborator at another university, onboarding a student, a journal-club pack),
and it exercises the code that copies a paper and its highlights between libraries, which Phase 1 needs
anyway. About a week.

### Comparison

| | Mac | Windows | Linux | iPhone/iPad | Android | Permissions enforced by | KI-approved storage | Lunta work |
|---|---|---|---|---|---|---|---|---|
| (a) Drive for desktop folder | yes | yes | no | – | – | Google (read-only share) | no (personal Google) | folder library |
| (a) Drive API | yes | yes | yes | yes | yes | Google | no | needs the restricted scope |
| (b) iCloud shared folder | yes | iCloud for Windows | no | yes | no | Apple (read-only share) | no | folder library + bookmark |
| (b) CloudKit share | yes | no | no | yes | no | Apple | no | second storage model |
| (c) OneDrive/SharePoint client | yes | yes | 3rd-party | Phase 2 (Graph) | Phase 2 (Graph) | Microsoft | **yes** | folder library |
| (d) WebDAV / Graph `RemoteStore` | yes | yes | yes | yes | yes | the server | Graph: yes | one backend each |
| (e) `.lunta` bundle | yes | yes | yes | yes | yes | – (a copy) | – | export/import |

## 3. Data model

### One library per group, several open at once

Two ways to model it:

1. **Several libraries open at once** (recommended). A group is a complete Lunta library in its own folder,
   opened with its own `Library::open(root, cache_dir, device_id)`. The core already supports this: `Library`
   holds no global state, and the desktop app already keys the search index by folder (`open_library`: FNV
   hash of the path → `libraries/<hash>/index.sqlite`). The apps keep a list of open libraries; the personal
   one is first.
2. *One library with a shared sub-library*: group papers as a kind of paper inside the personal library's
   state. Rejected: the personal library's logs and files would have to be split by sync rule (which files go
   to the group folder?), every `OpKind` would need a "where does this go" answer, and leaving a group would
   mean surgery on the personal library.

What "several at once" asks of the code:

- **Core**: nothing structural. Additions are below (identity, overlay, group file, faster replay).
- **Apple**: `LibraryModel` (`apple/Shared/LibraryModel.swift`) is one object for one library, with a
  static `cacheURL`; it becomes one instance per library (cache `…/libraries/<library id>/`), and a small
  `Libraries` registry holds them, their bookmarks and their sync (`CloudOpsWatcher` for the personal one, a
  folder watcher for groups). Most views take a `LibraryModel` already; the sidebar, search and "Add to" menus
  need to know about several.
- **Windows/Linux**: `AppState.open` (one `OpenLibrary`) becomes a map; Tauri commands take a library id.
- **Android**: `AppModel.rootDir` already names a working folder per mode (`library-drive`, `library-local`);
  a group is one more working folder with its own `LibrarySync` (Phase 2).
- **MCP** (`core/src/bin/lunta-mcp.rs`): opens one library from `library-root`. It reads a second file,
  `libraries.json` (written by the Mac app: id, name, root, read-only), adds `list_libraries`, and an optional
  `library` argument on every tool (default: personal). Writes into a group use an agent log in the group folder,
  so the group sees "added by Anna's agent". Off by default: an agent's writes are visible to 20 people.
- **Citations** (Word add-in, Google Docs, `desktop/src-tauri/src/citeserver.rs`, the Mac's endpoints): a citation
  stores the paper id; ids are UUIDs, unique across libraries, so lookup tries each open library. Cite keys are
  per library: the same paper in the personal and the group library may have two keys. A group's `.bib` file
  (`settings.bib_files`) is written from the group library, so the whole group cites the same keys.

### Identity: from devices to people

Today an op knows only its device (`Op { at, device, kind }`), and device ids are per install
(`mac-870c7b36`, `ios-0fa17bc3`, `agent-…`). A group needs people:

- A **person id** (random UUID) made once per person and kept in the personal library, so all of a person's
  devices know it: a new op in the personal log, `SetProfile { person, name }` (latest wins).
- When a device first writes to a group, the first line of its group log is `DeviceOwner { person, name }`.
  `State` keeps `devices: device → person` and `people: person → name` (latest `DeviceOwner` per person).
  Agent logs (`agent-…`) belong to the person whose app started them.
- **Who added what** is derived, not stored in the payload: `PaperState.added_by` = person of the device
  that wrote the first `AddPaper` (already the op that sets `added`); a highlight's author = person of the device
  that wrote its first `AnnotationUpsert`. Exposed as `Paper.added_by` and `Annotation.author` (UniFFI records;
  personal libraries leave them empty).
- Unknown ops are skipped by older versions (`serde_json::from_str::<Op>` fails on an unknown tag and the
  line is skipped in `Store::replay`), so new op kinds need no format version.
- Devices in a group could use their personal device id or a fresh one per group. A fresh one (stored with the
  group's bookmark) avoids linking one's device across groups and keeps the group's file names meaningful
  only inside it. Recommended.

### Does the op-log merge scale to 20 people?

Today's numbers, from the author's own library: 997 papers, 4 logs, 1,768 op lines, 2.4 MB of logs
(`AddPaper` lines carry the abstract, ~1.4 KB; a highlight upsert carries quote, prefix and suffix, ~0.6–1 KB).
A group: 20 people × (Mac, phone, tablet, agent) ≈ 60–80 logs; say 2,000 papers and 20,000 highlight ops a year:
roughly **20–30 MB of logs a year, 60–90 MB after three**.

- **Sync**: fine. Each log is written by one device and grows by appending; the sync clients move only what
  changed (OneDrive and iCloud upload changed files; the core's `Syncer` uploads a grown log whole, a few
  hundred KB per device).
- **Replay**: the weak spot. `Store::replay` reads and parses *every* log whenever *any* log changed, then
  sorts all ops; `Library::refresh` then runs `reindex_quick` over all papers. With 20 people someone changes
  something every few minutes; 60 MB of JSON is ~0.3–0.6 s on a Mac and several seconds on a phone, each time.
  Fix (core, before groups ship):
  1. **Incremental replay**: remember per log the byte offset read; on refresh read only the new tails. If all
     new ops are later (`(at, device)`) than the last op applied, apply them to the existing `State` (the
     common case); otherwise (a device that was offline for a week) replay fully.
  2. **A snapshot in the device's cache** (`State` + offsets) so opening a group does not parse three years of
     logs. Rebuilt whenever in doubt; the logs remain the truth.
  3. `reindex_quick` only for papers the new ops touched.
- **Clocks**: ops are ordered by the writing device's wall clock (`now_ms()` in `Store::append`). With one
  person's devices that is good enough; with 20 people one laptop whose clock is an hour fast wins every
  conflicting edit for that hour. Make `at` a hybrid logical clock: `max(now_ms, newest at seen + 1)`.
  One line in `Store::append` plus remembering the newest `at` in `replay`.

### Conflicts

The rules stay those of the personal library (newest op wins, per field or object):

- `SetMeta` replaces the whole `PaperMeta`: two people fixing different fields of one paper at the same time
  lose one fix. Rare with one person, plausible in a group. Later: a field-level `SetMetaFields`.
- Highlights: `AnnotationUpsert` replaces the whole annotation. In a group the apps let only a highlight's author
  change its colour or note (others can't edit it; see below for comments), so concurrent edits of one
  highlight come from one person.
- Tags (`Tag { present }`) and folder membership (`Member { present }`): per paper and tag/folder, newest wins.
  Folders (`CollectionUpsert`): newest name and parent win.
- Citation keys: already designed for devices that write before they see each other (`State::assign_cite_keys`:
  the earliest claim keeps a key).
- Shared root files (`smart-folders.json`, `watches.json`, `foryou.json`) are per-person things: a group library
  does not have them. A group's own settings are in `group.json` (owner) and `members/<person>.json` (each
  person their own), so no file is written by two people.

### Personal state about group papers: the overlay

Read / starred / Must Read are per person, but the ops that set them (`SetRead`, `SetStarred`, `SetMustRead`)
would mark a group paper read for everyone. And `positions/<device>.json` in the group folder would show the
group what everyone reads and how far. So these go into the **personal** library, keyed by the group paper:

- New op `ExternalFlags { library, paper, read, starred, must_read }` in the personal log; `State` keeps
  `external: (library, paper) → flags` (not a paper, so nothing else changes). The apps read flags for a group
  paper from there.
- Reading positions of group papers go into the personal library's `positions/<device>.json` (paper ids are
  UUIDs, so they don't collide with personal ones).
- Phase 3 adds private highlights the same way: `ExternalAnnotationUpsert { library, paper, doi, annotation }`.
  Highlights anchor by quote, prefix, suffix and offset in the reading view's text, and are found again in a
  reading view rebuilt with other spacing (`docs/development.md`, *Reading: one paper, two views*; the reader
  tests cover it), so a private highlight survives the group remaking the reading view. `doi` lets it follow
  the paper if the group deletes and re-adds it, or into the personal library ("Keep a Copy").

### Deletion

`DeletePaper` is permanent (the id goes into `State.deleted`; a later `AddPaper` of the same id is ignored) and
`Library::delete_paper` removes `papers/<id>/` at once, which the sync client passes on to everyone. In a group:

- The apps offer **Remove from Group** only to the person who added the paper and to the owner, with a
  confirmation that says what goes ("12 highlights by 4 people"). Others can remove it from a folder.
- Files are not deleted at once: a group library moves `papers/<id>/` to `.removed/<id>/` (still in the group
  folder, so it syncs) and deletes it after 30 days. The storage's own bin (OneDrive 93 days, iCloud "Recently
  Deleted" 30 days) is the second net.
- Deleting a folder (`CollectionDelete`) removes only the folder, never papers (as now).
- What an editor *can* still do outside Lunta (delete files in Explorer) is section 5.

## 4. UI sketch

**Sidebar (Mac; the desktop app the same; iPhone: a section in the library list)**

```
LIBRARY
  All Papers              997
  Must Read                12
  Folders ▸
  Tags ▸
GROUP: CASTELO-BRANCO LAB        (synced 2 min ago · OneDrive)
  All Papers              614
  New Since Last Visit      7     ← added by others since you last opened the group
  This Week's Journal Club  1
  Folders ▸
  People ▸   Anna L. (212) · Björn K. (88) · you (40) …
+ New Group Library…   Join Group Library…
```

**Paper list**: an "Added by" column (Mac table), "Anna · 3 days ago" under the title on phones; filter by
person under People. A paper also in the personal library (same DOI) shows a small mark, both ways.

**New Group Library…** (owner)

1. Name ("Castelo-Branco lab").
2. Where: "Choose a folder that your group can share: a OneDrive, SharePoint or Teams folder, an iCloud Drive
   folder, or a Google Drive folder synced by Google Drive for desktop." Folder picker; Lunta writes
   `group.json` (`{id, name, created, owner: person, files: "all" | "open-access" | "none"}`) and
   `members/<person>.json`.
3. **Invite People…**: Lunta can't share the folder itself (no API, by design), so it says how: "Share the
   folder 'Castelo-Branco lab – Lunta' in OneDrive with your group (Can edit for editors, Can view for readers),
   then send them this link." On the Mac, an iCloud Drive folder opens the system's share sheet for folder
   collaboration where the system offers it. The link is `lunta://join?group=<id>&name=Castelo-Branco%20lab`:
   it only tells Lunta what to look for; it grants nothing.

**Join Group Library…** (member): the link or the menu opens a folder picker ("Pick the shared folder
'Castelo-Branco lab – Lunta' in your OneDrive"). Lunta checks `group.json` (same id as the link), asks "How should
the group see you?" (prefilled from the personal profile), writes `DeviceOwner` and `members/<person>.json`, and
opens it. If the folder is read-only (a test write in `ops/` fails), it opens as a reader: no editing controls,
highlights only as private ones (Phase 3; before that: "Keep a Copy" to highlight in the personal library).

**Adding a paper to the group**: context menu **Add to Group ▸ Castelo-Branco lab** (or drag onto the group in
the sidebar). A sheet: "Also share your 5 highlights and notes" (on by default), and, if the group shares only
open-access files and the paper's `license` isn't CC-*, "The PDF stays in your library; the group gets the
details and your highlights". The core copies metadata, files, tags and chosen highlights into the group (merging
by DOI with a paper already there: `add_paper` does that), keeping the personal copy. The reverse, **Keep a
Copy in My Library**, from a group paper.

**Highlights in a group paper**

- A note at the top of the reader the first time: "Highlights here are seen by the Castelo-Branco lab."
- Toolbar: **Show ▸ Everyone's / Mine / Anna L. / Björn K. …**. Others' highlights show the author's initials in
  the margin (Mac, desktop) or in the highlight's sheet (phones); the colour stays the highlight's colour.
- The highlight editor shows "Anna L., 12 Nov"; others' highlights are read-only (Copy Quote, Reply in Phase 3).
- Phase 3: a **Private** switch in the editor and the selection menu (Highlight · Private Highlight), and a
  per-group default ("New highlights: Shared / Private").
- Highlights & Notes, Outline and Summary get the same person filter, so the journal-club presenter can make an
  outline of everyone's highlights.

**Leaving**: Group ▸ **Leave Group…**: Lunta stops opening the folder and forgets its bookmark; "Your 40 papers
and 130 highlights stay in the group." Optional **Remove My Name**: `members/<person>.json` becomes "Former
member". The person stops the folder's sharing in OneDrive themselves (or the owner removes them).

## 5. Security, privacy and GDPR

**What Lunta can and cannot enforce.** Without a server, the only enforced permissions are the storage's:
whoever can write to the folder can write any file in it, including someone else's log, and delete anything.
So:

- *Readers*: give them read-only access in OneDrive / iCloud / Drive. That is real enforcement.
- *Editors vs owner*: enforced by the apps only (who may remove papers, edit others' highlights, rename the
  group). Honest clients follow it; a member with a text editor does not have to. Phase 3 can make tampering
  *visible*: the owner signs `group.json` (an Ed25519 key kept in the owner's personal library), members'
  devices sign their log lines, and the apps ignore (and flag) ops from devices not in the signed member list or
  lines that don't verify. This detects a removed member still writing, or one member writing in another's log;
  it cannot stop deletions, which the storage's versioning and bin undo.
- Write this in the app's help, not only here: a group library is as private as its folder.

**GDPR.** Highlights and notes are personal data (they carry a name and show what someone reads and thinks);
"added by", reading positions and read flags are too.

- *Controller and processor*: the group's work data is KI's (KI is the controller); the storage provider is
  KI's processor if it's KI's Microsoft 365. Lunta's developer processes nothing: no server, no telemetry. That
  is the main argument for option (c) at KI and against personal Google Drive or iCloud accounts for group data:
  KI's agreements cover its Microsoft 365, not a doctoral student's private iCloud. Check with KI IT and the data
  protection officer (dataskyddsombud) which classes of information may go where; Lunta's documentation should
  point groups there rather than decide.
- *Minimisation*: names only (no e-mail addresses in the library); read flags and positions stay personal
  (the overlay, section 3); device ids per group.
- *Transparency*: the join sheet says what the group sees: your name, papers you add, your shared highlights and
  notes, when.
- *Erasure and leaving*: a member's contributions live in their own logs. Deleting a log erases them but also
  the papers they added for everyone. So: **Remove My Name** (pseudonymise; contributions stay) and, for full
  erasure, an owner action **Take Over Contributions…** that re-writes a person's papers (not their notes) under
  the owner's device, after which that person's logs can be deleted and their notes are gone. Old copies remain
  in the storage's version history until it expires.
- *Sensitive data*: notes are free text. The group's onboarding text should say: no patient data or
  pseudonymised research data in notes.
- *AI*: Ask and Summary run on the device (Apple's model, Qwen3), nothing leaves it. An MCP agent (Claude
  Desktop, Claude Code) sends what it reads to its provider: for a group library that is other people's notes.
  MCP access to group libraries is off by default and per group (section 3).

**Licences of PDFs.** A group folder copies PDFs (and the reading views made from them, which are copies too) to
every member. Most publisher licences let *authorised users* of a subscribing institution (KI staff and students)
share with other authorised users for research and teaching; they do not cover collaborators at other
universities or companies who are in the group, and terms differ by publisher (in Sweden, the Bibsam
consortium's agreements). Open-access papers under CC licences are fine. So: the group setting **Files:
All / Open access only / None** (`PaperMeta.license` already holds the licence string), and with "None" or a
non-open paper each member's own copy (matched by DOI) supplies the PDF; highlights still anchor, because they are
found by their quote. The default for a new group: All, with one sentence on what that means.

## 6. Phased plan

Estimates are for one developer working with an agent, as the rest of Lunta has been built; they include
tests and docs, not App Store review.

### Phase 0 (optional, ~1 week): `.lunta` bundles

Export a folder or a selection as a bundle; import it into any library. Core: `Library::export_bundle(ids,
include_highlights) -> zip`, `Library::import_bundle(path)` (by DOI into existing papers; highlights re-anchored
by quote). Mac, desktop, iPhone (Share sheet), Android. Useful alone, and Phase 1's "Add to Group" uses the same
copy code.

### Phase 1 (~8–10 weeks): a group folder opened next to the personal library — Mac, Windows, Linux, and iPhone/iPad through an iCloud shared folder

Scope:

- Open, create and join a group library in a folder: OneDrive / SharePoint / Teams or Dropbox through their
  desktop clients, Google Drive for desktop, iCloud Drive shared folders, a network share. Read and write.
- Shared papers, PDFs, reading views, full texts, tags, folders, citation keys, highlights and notes (all
  highlights in a group paper are shared). "Added by" and highlight authors; People filter; New Since Last Visit.
- Personal read / starred / Must Read and positions for group papers (overlay).
- Add to Group (with highlights), Keep a Copy, Remove from Group (adder and owner), `.removed/` grace period.
- Read-only folders open as readers.
- MCP: `list_libraries`, `library` argument, group access off by default.
- Not in Phase 1: Android, iPhone/iPad with OneDrive or Google Drive, private highlights, signatures.

Core changes:

| What | Where |
|---|---|
| Hybrid logical clock for `at` | `Store::append`, `Store::replay` (newest `at` seen) |
| Incremental replay + cached snapshot; `reindex_quick` for touched papers only | `Store` (per-log offsets), `Library::refresh`, `Library::open` |
| `DeviceOwner`, `SetProfile` ops; `State.devices`, `State.people`; `PaperState.added_by`; annotation authors | `store.rs` `OpKind`, `State::apply` |
| `Paper.added_by`, `Annotation.author`, `Library::people()` | `model.rs`, `lib.rs` (UniFFI) |
| `ExternalFlags` op and `State.external`; `Library::external_flags`, `set_external_flags` | `store.rs`, `lib.rs` |
| `group.json` / `members/` read and written; `Library::group_info()`, `is_read_only()` | new `core/src/group.rs` |
| `copy_paper_to(target: &Library, id, with_highlights)` (DOI merge, files, tags, highlights with new ids) | `lib.rs` (shared with Phase 0) |
| Group delete: `.removed/<id>/`, purge after 30 days | `Library::delete_paper` for group libraries |
| MCP: `libraries.json`, `list_libraries`, `library` argument | `core/src/bin/lunta-mcp.rs` |

App changes: Mac and iPhone/iPad: `Libraries` registry, `LibraryModel` per library (per-library cache),
security-scoped folder bookmarks, folder watcher, sidebar section, join/create/invite sheets, Add to Group,
author marks and the Show filter in the readers (`ReaderController`, `Highlights.swift`, `PDFHighlights.swift`),
People in Highlights & Notes / Outline. Windows/Linux: `AppState` with several libraries, the same UI in
TypeScript. Citation lookup across libraries in both citation servers.

Rough split: core 2–2.5 weeks, Mac 2.5 weeks, iPhone/iPad 1.5 weeks (mostly shared views), Windows/Linux 1.5–2
weeks, MCP and citations 0.5 week, docs and a two-person trial 1 week.

### Phase 2 (~6–8 weeks): phones on every storage

- `RemoteStore` backends: **Microsoft Graph** (OneDrive/SharePoint: delta API for listing, upload sessions for
  big files; OAuth with PKCE like Google's; an Entra app registration, and KI's tenant may require admin consent
  for third-party apps with `Files.ReadWrite.All`, which can take weeks: start that early) and **WebDAV**
  (Nextcloud, SUNET Drive; app passwords). The `Syncer` is unchanged: each group is a working folder plus its
  own `Syncer` and state, as Android and Drive mode on iPhone already do for the personal library.
- Android: groups through Graph or WebDAV (iCloud is not possible). iPhone/iPad: groups on OneDrive or WebDAV
  through the API, iCloud shared folders as in Phase 1.
- Google Drive groups through the API only if Lunta accepts the restricted scope for that feature (an
  opt-in "Use a shared Google Drive folder" asking for `drive.readonly` on top of `drive.file`, with CASA). Until
  then, Google Drive groups are desktop-only.
- Lazy downloads (`SyncOptions::lazy`, `fetch_paper`) on by default for groups on phones: a group of 600 papers
  is several GB.

### Phase 3 (~5–6 weeks): permissions and private highlights

- Private highlights in group papers (`ExternalAnnotationUpsert` in the personal library), the Private switch
  and group default; readers can then highlight too.
- Replies to a highlight (a new op, so others can respond without editing it).
- Signed `group.json` and signed log lines; ops from unknown devices ignored and flagged; owner transfer.
- Take Over Contributions and Remove My Name; field-level metadata edits (`SetMetaFields`).

### Risks

- **Sync clients and append-only files.** OneDrive, Dropbox and Google Drive for desktop are built for
  documents, not logs appended every few seconds; a client may upload a log mid-write (handled: a partial last
  line is skipped) or make a "conflicted copy" if two of a person's own devices were somehow given the same device
  id (handled by unique ids; the apps should ignore `* (1).jsonl`-style copies and say so). Needs a real trial
  with three people on OneDrive before anything else.
- **iCloud shared folders from a sandboxed app**: bookmarks to a shared folder outside the container, eviction,
  and the watcher. Spike first (2–3 days).
- **Replay cost on phones** if incremental replay is not done first.
- **Scope creep into a server**: real-time presence, comments with notifications, access revocation. Out of
  scope by principle; say so in the docs.
- **KI policy**: if KI says group research data belongs only in its Microsoft 365, Phase 1's iCloud path is for
  other groups, and phones wait for Phase 2's Graph backend and the tenant's consent.
- **Support load**: "I can't see Anna's papers" will usually be the storage's sharing, not Lunta. The group view
  should show per person when their last change arrived ("Anna L.: last change 3 days ago"), so it's clear whose
  device isn't syncing.

## 7. Recommendation

Build group libraries as **ordinary Lunta libraries in a folder the group already shares, opened next to the
personal library**. The per-device logs were made for several writers that never touch the same file, and that
property carries from one person's devices to a lab without a server. Concretely:

1. First, in the core: hybrid logical clock and incremental replay (worth having even without groups), then
   people (`DeviceOwner`, `added_by`, authors) and the personal overlay for read flags and positions.
2. **Phase 1 on Mac, Windows, Linux and iPhone/iPad, with KI's OneDrive / SharePoint / Teams as the
   recommended storage** (approved by the university, read-only sharing for readers, version history), iCloud
   shared folders for all-Apple groups, Google Drive for desktop for groups on Google. Do Phase 0 (bundles) along
   the way: small, useful alone, and it builds the copying.
3. Phones on OneDrive and WebDAV in Phase 2 through new `RemoteStore` backends; start KI's tenant consent for a
   Graph app registration at the start of Phase 1.
4. Do **not** build group sync on the Drive API with `drive.file`: a file another member's Lunta created is
   invisible to yours. Do not build CloudKit sharing or a relay.
5. Permissions are the storage's (readers) plus honest clients (owner vs editors) until Phase 3 makes tampering
   visible with signatures. Say this plainly in the app.

Before writing code: a three-person trial of the existing Windows app on three PCs pointed at one OneDrive
shared folder for a week (it works today, minus authors and personal read flags; the Mac app can't open an
arbitrary folder yet), and a 2–3 day spike on an iCloud shared folder from the sandboxed Mac and iPhone apps.
