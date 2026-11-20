# Lunta on Google Play

How the Android app gets onto Google Play: the build, the signing key, the CI upload and, step by step, what to
fill in in Play Console. The APK on GitHub stays as it is (with its updater); the Play build is the same app
without it.

Contents: [The Play build](#the-play-build) · [Signing: one key for GitHub and Play](#signing-one-key-for-github-and-play) ·
[CI](#ci) · [Play Console, step by step](#play-console-step-by-step) · [Store listing](#store-listing) ·
[App content (policy forms)](#app-content-policy-forms) · [Closed testing and production access](#closed-testing-and-production-access) ·
[Graphics and screenshots](#graphics-and-screenshots) · [Uploading from CI](#uploading-from-ci) · [Later releases](#later-releases)

## The Play build

Not a product flavor: the existing build switch `lunta.selfUpdate` (`android/app/build.gradle.kts`). With
`LUNTA_SELF_UPDATE=false` (or `lunta.selfUpdate=false` in `local.properties`):

- `BuildConfig.SELF_UPDATE` is false, so `Updater` never contacts GitHub and Settings has no Updates section;
- `src/selfupdate/AndroidManifest.xml` is not merged: no `REQUEST_INSTALL_PACKAGES` (Play allows it only for app
  stores, file managers and the like) and no FileProvider for downloaded APKs.

```sh
cd android
LUNTA_SELF_UPDATE=false ./gradlew :app:bundleRelease     # → app/build/outputs/bundle/release/app-release.aab
./gradlew :app:assembleRelease                           # the GitHub APK, unchanged
```

Gradle refuses `bundle*` without `LUNTA_SELF_UPDATE=false`: an AAB is only for Play, and one with the updater
would be rejected. Everything else is shared: application id `app.lunta`, the release key, the version name and
the version code (`LUNTA_VERSION_CODE`, in CI the release workflow's run number, so it only grows: v1.1.0 was 14).
Upload only the AAB that CI builds for a tag; a bundle uploaded by hand with a made-up larger version code would
block every later CI build.

Manifests (checked with `aapt2 dump permissions` on the APK and the merged manifest of the bundle):

| Permission | GitHub APK | Play AAB | Why |
|---|---|---|---|
| `INTERNET`, `ACCESS_NETWORK_STATE` | yes | yes | lookups, PDFs, Drive sync, downloads of the voice and the AI model |
| `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MEDIA_PLAYBACK` | yes | yes | Listen (`ListenService`, type `mediaPlayback`) |
| `POST_NOTIFICATIONS` | yes | yes | the New for You morning notification (asked on Android 13+) |
| `WAKE_LOCK`, `RECEIVE_BOOT_COMPLETED` | yes | yes | added by WorkManager (sync, New for You in the background) |
| `app.lunta.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION` | yes | yes | added by AndroidX (core) for its own receivers |
| `REQUEST_INSTALL_PACKAGES` | yes | **no** | the GitHub updater |

No `QUERY_ALL_PACKAGES` (the only `<queries>` entry is Custom Tabs), no location, contacts, storage or media
permissions (PDFs come through the system file picker and the share sheet), no advertising ID.

Play's technical requirements, checked:

- **Target API:** `targetSdk = 36`, above Play's current requirement.
- **64-bit:** `arm64-v8a` and `x86_64` only (no 32-bit libraries at all, so nothing to pair).
- **16 KB pages** (required for apps targeting Android 15+): every `.so` in the bundle (the core, PDFium,
  sherpa-onnx, ONNX Runtime, JNA, AndroidX's) has 16 KB-aligned LOAD segments (`llvm-readelf -l`). The libraries
  are compressed in the bundle (`useLegacyPackaging`, extracted at install, which PDFium's loading by path needs),
  so zip alignment doesn't matter.
- **Size:** the bundle is about 95 MB with both ABIs; Play delivers one ABI, about 21 MB of compressed native
  libraries plus code and resources, far below the 200 MB limit for the base module. The AI model (1.3 GB) and
  the natural voice (350 MB) are downloaded inside the app when the user asks, as now.
- **Self-updating code:** the updater classes are still in the code but switched off by the build constant;
  the Play build never downloads or installs code.

## Signing: one key for GitHub and Play

Play installs only apps signed with the key Play holds (Play App Signing; every new app must use it). Android
installs an update only if it is signed with the same key as the installed app. So:

**Recommended: (a) the same application id, and the existing release key as Play's app signing key.** You give
Play a copy of `~/.lunta/lunta-android-release.jks` (encrypted for Google with their PEPK tool), and Play signs
the APKs it delivers with that same key. Then the GitHub APK and the Play version are the same app to Android:

- someone who installed the APK from GitHub can install Lunta from Play over it (or the other way round)
  and keeps the library, the Drive sign-in and the settings;
- the APKs on GitHub keep working and updating exactly as now; the key's SHA-256 stays
  `E3:B3:4F:21:…:F6:C9:BB:41` everywhere (the Google sign-in client is the "iOS"-type client, which doesn't
  depend on the key anyway);
- the same key can also be the **upload key** (what CI signs the AAB with before Play re-signs it). A separate
  upload key would normally protect the app signing key, but here the app signing key is in GitHub's secrets
  anyway for the APK, so it adds a second secret without making anything safer.

Trade-offs of (a):

- Google holds a copy of the key. You can't take it back, and you must never lose your own copy (Bitwarden):
  Play can't give it back, and without it no GitHub APK could be updated again.
- Play's "key upgrade" (rotating to a new key for Play installs) would split the two channels again. Don't use it.
- If the key ever leaked, both channels would be affected at once (as the GitHub APK is today).

(b) A different application id for Play (e.g. `app.lunta.play`) with a Play-generated key would keep the key
only with you, but the two would be different apps: both could be installed side by side with separate
libraries, nobody could move from the APK to Play without setting up again, and every place that names
`app.lunta` (OAuth redirect, providers, docs) would need a second value. Not worth it.

The steps for (a) are in [Play Console, step by step](#play-console-step-by-step) (step 5).

## CI

`.github/workflows/android.yml`, for a release (a `v*` tag, through `release.yml`):

1. decodes the key from `ANDROID_KEYSTORE_BASE64` / `ANDROID_KEYSTORE_PASSWORD` once (removed when the step
   ends, also on failure);
2. builds the GitHub APK (`:app:assembleRelease`, updater on) and then the Play bundle
   (`LUNTA_SELF_UPDATE=false :app:bundleRelease`), both with the same key, version name and version code;
3. attaches `Lunta-<version>-Android.apk` and `Lunta-<version>-Google-Play.aab` to the release (the release notes
   say the `.aab` is not for phones);
4. only if the secret `PLAY_SERVICE_ACCOUNT_JSON` exists, uploads the bundle to Play (see
   [Uploading from CI](#uploading-from-ci)); otherwise the step says so and is skipped.

Nothing else changed: pushes to `main` and pull requests still build only the debug APK.

## Play Console, step by step

Do these once the account's identity and device verification are done. Play Console:
<https://play.google.com/console>.

1. **Create app.** All apps › Create app. App name **Lunta**; default language **English (United States) – en-US**;
   **App**; **Free**; tick the declarations (Developer Program Policies, US export laws). Free can't later
   become paid; that's fine.
2. **Store settings** (Grow users › Store presence › Store settings): category **Productivity** (see
   [Category](#category)); tags (up to five; e.g. Reader, Note taking, Productivity tools, if offered); contact
   details: email = your developer email, website `https://getlunta.app`, no phone.
3. **Main store listing** (Store presence › Main store listing): the texts and graphics below.
4. **App content** (Policy and programs › App content): every form in
   [App content](#app-content-policy-forms). Play won't let a release go out until all are done.
5. **App signing with the existing key.** Make the first release (Test and release › Testing › Closed testing ›
   create a track, or Internal testing to try first) › Create new release. Under *App integrity / Play App
   Signing* choose **Change app signing key** (or "Use a different key") › **Export and upload a key from Java
   keystore**:
   1. download the **PEPK tool** (`pepk.jar`) and the **encryption public key** (`encryption_public_key.pem`)
      from that page, into an empty folder;
   2. run, with the key file where it is:
      ```sh
      java -jar pepk.jar --keystore="$HOME/.lunta/lunta-android-release.jks" --alias=lunta \
        --output=lunta-play-signing-key.zip --include-cert --rsa-aes-encryption \
        --encryption-key-path=encryption_public_key.pem
      ```
      It asks for the keystore password and then the key password (both are the password saved in Keychain
      Access as "Lunta Android release key");
   3. upload `lunta-play-signing-key.zip` there and **Save**; do not register a separate upload key (the same key
      uploads); then delete the zip and `pepk.jar`.

   Play accepts RSA keys of 2048 bits or more. To check the key's size without changing anything:
   `keytool -list -v -keystore ~/.lunta/lunta-android-release.jks -alias lunta | grep -i "key algorithm\|SHA256"`.
6. **Upload the bundle.** Download `Lunta-<version>-Google-Play.aab` from the newest release (it exists from the
   first tag pushed after this change; v1.1.0 has none) and drop it into the release. Release name: the version;
   release notes: a few lines in `<en-US>…</en-US>`. Then check Test and release › Setup › **App signing**: the
   *App signing key certificate* SHA-256 must be `E3:B3:4F:21:4A:50:95:CF:F7:6A:4F:AA:19:CE:1B:51:A8:EF:CD:42:8A:45:C2:55:14:D1:AD:21:F6:C9:BB:41`,
   and the upload key the same. If it shows anything else, stop: Play generated its own key, and (a) didn't take.
7. **Testers and review:** [Closed testing](#closed-testing-and-production-access). Countries: all (or
   start with your own); the app has no country-specific content.

## Store listing

### App name (≤ 30)

**Lunta** — or, to say what it is in search results, **Lunta: Scientific Papers** (24). Play doesn't allow
"best", "free", "#1", emoji or prices in the name.

### Short description (≤ 80)

> Read, highlight and cite scientific papers. Private, free and open source.

(74 characters.)

### Full description (≤ 4000)

Prose, no lists of keywords or of platforms (stores reject those as spam), only what the Android app does.
About 2,000 characters:

> Lunta is a reader and reference manager for scientific papers, made by a researcher for reading papers on the go.
>
> Add a paper by its DOI, PubMed ID or arXiv link, share one to Lunta from your browser, or open a PDF. Lunta looks up its details and, when the paper is open access, finds the legal full text and turns it into a clean reading view that fits your phone: real text instead of a shrunken page, with figures you can open full size, equations and the reference list. The original PDF is always one tap away.
>
> Highlight passages in four colours and add notes to them. Each paper keeps a list of its highlights, and your reading position is remembered. Organise papers in folders and with tags, star the ones that matter, and search the full text of everything you have saved.
>
> New for You brings new papers for your research every morning, picked from the topics, journals and people you follow. Add your ORCID iD and it also learns from your own papers and shows you new work that cites them.
>
> Listen reads a paper aloud in a natural voice that runs on your phone, following along paragraph by paragraph and carrying on from the lock screen or with your headphones. An optional on-device AI model summarises a paper and answers your questions about it without sending anything anywhere.
>
> When you write, copy a reference in APA, Vancouver or Nature style, or share papers as BibTeX or RIS for your reference manager or LaTeX.
>
> Your library is yours. Keep it on your phone, or connect your own Google Drive to keep it in a Lunta folder there and in sync with Lunta on your computer. There is no Lunta account and no Lunta server: the developer never sees your papers, highlights or notes. To find papers, Lunta asks public scholarly services such as Crossref, OpenAlex and PubMed for what you look up, and nothing else.
>
> Lunta is free and open source, with no ads, no tracking and no in-app purchases. The name is Swedish for a thick bundle of papers.

Check before pasting that each feature named is in the build being released (they are in 1.1). Don't mention
other apps (Zotero, Mendeley…) by name in the listing.

### Category

**Productivity** (recommended): Lunta is a working tool (a library, reading, citing) and competes with reference
managers and readers there; "Education" fits course and learning apps and invites families/kids scrutiny Lunta
doesn't need. Either is allowed.

### Contact and links

- Email: your developer email (Play shows it publicly on the listing).
- Website: `https://getlunta.app`
- Privacy policy (App content › Privacy policy): `https://getlunta.app/app-privacy.html`

## App content (policy forms)

Answers consistent with the [privacy policy](https://getlunta.app/app-privacy.html).

| Form | Answer |
|---|---|
| Privacy policy | `https://getlunta.app/app-privacy.html` |
| App access | **All functionality is available without special access.** No Lunta login; Google Drive sign-in is optional (a library on the phone works without it). If Play asks for credentials anyway: none needed, choose "a library on this phone" on first start. |
| Ads | **No**, the app contains no ads. |
| Content rating | the questionnaire below |
| Target audience and content | Age groups **18 and over** only. Appeals to children: **No**. (Not for children: it's a research tool; the privacy policy says so.) |
| News app | **No** |
| Health apps | **No** health features (reading medical papers is not a health feature) |
| Financial features | **None** |
| Government app | **No** |
| Advertising ID | **No** (Lunta doesn't use it; the manifest has no `AD_ID`) |
| Data safety | below |
| Foreground service permissions | below |
| Sensitive permissions | none: no SMS, call log, location, all-files access, `QUERY_ALL_PACKAGES`, accessibility, exact alarms or `REQUEST_INSTALL_PACKAGES` in the Play build |

### Content rating (IARC questionnaire)

Email: your developer email. Category: **Utility, Productivity, Communication, or Other** (not a game, not a
social app). Then:

- Violence, fear, sexuality, nudity, crude humour, language, drugs, alcohol, tobacco, gambling, simulated
  gambling: **No** to all.
- Does the app allow users to interact or exchange content with other users (chat, sharing with others,
  user-generated content seen by others)? **No.** (Sync is between one person's own devices through their own
  Drive; sharing a citation goes through Android's share sheet.)
- Does it share the user's current physical location with others? **No.**
- Does it allow purchases of digital goods? **No.**
- Does it contain or let users access unrestricted web content (a web browser)? **No.** (Links open in the
  phone's own browser; the reader shows the papers the user adds.)
- If asked whether the app generates content with AI: **Yes**, on-device text (a paper's summary, answers about
  a paper), shown only to that user, from the paper's own text. Play's AI-generated content policy asks apps
  that generate content to let users report offensive output; if review raises it, the answer is a "Report"
  item on the Summary card and in Ask (not built yet).

Expected rating: Everyone / PEGI 3 / USK 0.

### Data safety

The question Play asks is what the **app** sends off the device, even if the developer never receives it.
Lunta sends nothing to its developer, but with Google Drive connected it does upload the library to the user's
own Drive, and it sends search words and identifiers to public scholarly services. Recommended answers (honest,
and on the safe side of Play's definitions):

- Does your app collect or share any of the required user data types? **Yes.**
- Is all of the user data collected by your app encrypted in transit? **Yes** (HTTPS only; Android refuses
  cleartext for targetSdk 28+ and Lunta doesn't opt out).
- Do you provide a way for users to request that their data is deleted? **Yes**: the data is in the user's own
  Drive and on the phone; deleting the Lunta folder in Drive (or the app) deletes it. (The developer holds none,
  so there is nothing to request from us; say that in the field if asked for a URL: the privacy policy.)

Data types:

| Data type | Collected | Shared | Optional? | Purpose | Ephemeral? |
|---|---|---|---|---|---|
| Files and docs (papers, PDFs) | yes, if Drive is connected | no | optional | App functionality | no |
| Other user-generated content (highlights, notes, folders, tags) | yes, if Drive is connected | no | optional | App functionality | no |

"Collected" here means uploaded to the user's own Google Drive at the user's choice; it isn't shared with third
parties for their own use. Not declared:

- searches, DOIs, followed topics and an ORCID iD sent to OpenAlex, Crossref, PubMed and the like: public
  identifiers and search words sent to look them up, at the user's request, with no account or device id and
  nothing kept by Lunta (Play's exemptions for user-initiated transfers and ephemeral processing). If you prefer
  to be stricter, add *App activity › In-app search history* as collected, ephemeral, App functionality, not
  shared;
- downloads (the voice, the AI model, open-access PDFs, update checks are not in the Play build): nothing about
  the user is sent;
- crash reports, analytics, device ids: Lunta has none.

The alternative, "No data collected", rests on the reading that the user's own Drive isn't collection by the
app. If you choose it, keep the privacy policy's wording ("the developer of Lunta has no access to it") as the
justification. The recommended answers above are what the policy describes, so either is consistent.

Note for the privacy policy page: it still says Google asks to "see and edit the files in your Drive (Google's
standard Drive permission)". Since the move to `drive.file` that should read "see, edit, create and delete only
the specific Google Drive files you use with Lunta"; and the update paragraph could add that the Android app from
Google Play is updated by Google Play. Worth fixing before review, as reviewers compare the policy with the app.

### Foreground service permissions

Play asks, for apps targeting Android 14+, why each foreground service type is used. Lunta uses one:

- Type: **Media playback** (`FOREGROUND_SERVICE_MEDIA_PLAYBACK`, `ListenService`, a media3
  `MediaSessionService`).
- Description: "Listen reads a paper aloud. The user starts it from the paper's ⋯ menu › Listen; playback goes on
  with the screen off or in another app, with the media notification and lock-screen and headset controls, until
  the user pauses or stops it."
- User impact if deferred or interrupted: playback would stop as soon as the screen turns off.
- **Video link** (required): an unlisted YouTube video or a Drive link anyone can open, 30 to 60 seconds, from a
  real phone's screen recording: open a paper › ⋯ › Listen › playback starts and the paragraph is highlighted ›
  press Home › pull down the notification (the media controls) › lock the phone and show the lock-screen
  controls › pause from there.

WorkManager's own `SystemForegroundService` is in the manifest without a type and Lunta never starts it in the
foreground; nothing to declare for it. `POST_NOTIFICATIONS` needs no form.

## Closed testing and production access

Personal developer accounts created after November 2023 must run a **closed test with at least 12 testers who
have been opted in continuously for the last 14 days** before they can apply for production. Testers who leave
early don't count, so recruit 15 to 20.

1. **Testers list.** Easiest is a Google Group: create one at <https://groups.google.com> (e.g.
   `lunta-testers@googlegroups.com`; who can join: anyone with the link, or by invitation). Alternatively an
   email list in Play Console (Test and release › Testing › Closed testing › Testers › Create email list); each
   tester's address must be the Google account on their phone. Jorge (Android) is one.
2. **Closed testing track.** Test and release › Testing › Closed testing › Create track (or use "Closed testing –
   Alpha"): Countries; Testers: tick the Google Group (enter its address) or the email list; *Feedback URL or
   email*: the GitHub issues page or your developer email. Create a release with the AAB (step 6 above), review
   and **Start rollout to Closed testing**. The first review of a new app takes a few days.
3. **Opt-in link.** On the Testers tab, once the release is reviewed: *Join on the web*,
   `https://play.google.com/apps/testing/app.lunta`. Send testers two things: the group's join link (if a group),
   then the opt-in link: they click **Become a tester**, then **Download it on Google Play** (or search the Play
   Store on the phone, signed in with that account). Testers who already have the GitHub APK installed get the
   Play version over it (same key) without losing anything.
4. **During the 14 days.** Testers should keep the app installed, open it now and then and send feedback; Google
   looks at engagement. Push at least one update to the track during the test (a new tag; CI attaches the AAB, or
   uploads it with the service account) and note what changed because of feedback: the application asks.
5. **Apply for production.** Dashboard › *Apply for production* (appears after the 14 days). The questionnaire
   asks, in prose: how you recruited testers; how easy that was; how testers used the app (features they tried);
   what feedback they gave and how you collected it; who the app is for (researchers, students, clinicians
   reading the literature) and what value it gives; how many installs you expect in the first year; what you
   changed because of the test; how you decided it was ready. Answer specifically (features, numbers, changes).
   Review takes up to about 7 days. After approval, Production › Create new release (promote the tested release).

## Graphics and screenshots

Made from the Lunta logo (`design/logo.svg`) and the site's palette and fonts, in `appstore/google-play/`
(untracked, like the other store assets):

| File | Play's slot | Spec |
|---|---|---|
| `icon-512.png` | App icon | 512 × 512 PNG, full square (Play rounds the corners itself), under 1 MB |
| `feature-graphic-1024x500.png` | Feature graphic | 1024 × 500 PNG without transparency; logo, "Lunta", "Read, highlight and cite scientific papers. Private, free and open source." on the teal gradient with a copper rule. No device frame or paper content |

**Phone screenshots** (2 to 8; 9:16 portrait, at least 1080 px wide so Play can feature them; a real phone, light
theme, status bar tidy: full battery, no notifications). Use a screenshot library, not your own: the Kukanja et
al. paper (CC BY; `10.1016/j.cell.2024.02.030`) for everything that shows a paper's content, and for lists the
open-access screenshot library used for the App Store (`appstore/screenshots/README.md`) or a few of your own
papers. Capture with the phone's own screenshot or `adb exec-out screencap -p > 01.png`:

1. **Library**: All papers, with stars, a few tags and read/unread, a highlight count visible.
2. **Reading view**: Kukanja et al. scrolled to a paragraph with a yellow and a blue highlight, one with a note.
3. **Figure**: a figure of Kukanja et al. opened full screen with its caption.
4. **New for You**: the morning feed with "In short" and a few cards (the topics you follow in the header).
5. **Listen**: the player under the reading view mid-paragraph (the paragraph highlighted), or the lock screen
   with Lunta's media controls.
6. **Summary / Ask**: the Summary card of Kukanja et al., or an answer in Ask about it (on-device model).
7. **Cite**: a paper's page with the APA / Vancouver / Nature citations, or the share sheet with BibTeX.
8. *(optional)* **Highlights list** of the paper, or Settings › Sync showing Google Drive connected.

Tablets (7" and 10") are optional; without them the listing just doesn't appear as "designed for tablets". A
Chromebook or Android XR slot isn't needed.

## Uploading from CI

Optional: with it, each tag's AAB also lands in a Play track as a draft (or rolled out). Without the secret
nothing changes.

1. **Enable the API.** Google Cloud Console (<https://console.cloud.google.com>), in the project Lunta already
   uses for Drive or a new one: APIs & Services › Library › **Google Play Android Developer API** › Enable.
2. **Service account.** IAM & Admin › Service accounts › Create: name `lunta-play-upload`, no roles in Cloud.
   Open it › Keys › Add key › JSON: a file downloads. (If the organisation policy blocks keys, that's a Workspace
   setting; a personal project has none.)
3. **Give it access in Play.** Play Console › Users and permissions › Invite new users: the service account's
   email (`…@….iam.gserviceaccount.com`) › App permissions › Lunta › **Release apps to testing tracks** (and, if
   CI should also do production, *Release to production*); Account permissions: none. Invite.
4. **Secret.** GitHub › the repository › Settings › Secrets and variables › Actions › New repository secret:
   `PLAY_SERVICE_ACCOUNT_JSON` = the whole JSON file's content. Then delete the downloaded file.
5. **Variables** (same page, Variables tab), both optional:
   - `PLAY_TRACK`: `internal` (default), `alpha` (closed testing's default track), `beta`, `production`, or a
     custom closed track's name;
   - `PLAY_RELEASE_STATUS`: `draft` (default: the release waits in Play Console for you to roll it out) or
     `completed` (rolled out at once). Keep `draft` until the app has had its first release reviewed: Play
     accepts only drafts for an app that has never been published.

The upload uses `r0adkll/upload-google-play`, pinned to the v1.1.5 commit. The first bundle must go up by hand
(step 6) anyway, so that Play App Signing is set up with the existing key before anything else is uploaded.

## Later releases

1. Push a tag as always (`git tag v1.2.0 && git push origin v1.2.0`). CI attaches the APK and the AAB (and, with
   the secret, uploads the AAB to `PLAY_TRACK`).
2. In Play Console, open the release on its track (or Create new release › *Add from library* / upload the AAB),
   write the release notes, roll it out; promote from testing to production when it's good.
3. Pre-release tags (`v1.2.0-1`) produce an AAB too; upload those to a testing track only.

Users who installed from GitHub keep the GitHub updater; users who installed from Play get Play's updates. Both
are signed with the same key, so either can switch at any time.
