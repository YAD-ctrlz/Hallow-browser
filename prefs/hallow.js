/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// Hallow's default preferences. They are appended to the branding pref file,
// which Gecko reads after firefox.js, so they replace Firefox's defaults
// while staying changeable in about:config and Settings.
// `cargo hb lint-prefs` checks that each one still exists upstream.

// ---- Clean UI --------------------------------------------------------------

// Compact density, with the option kept visible in Customize.
pref("browser.compactmode.show", true);
pref("browser.uidensity", 1);

// Toolbar: back, forward, reload, a full-width address bar, downloads.
// Tab strip: tabs, new tab, and the tab list (shown only when tabs overflow,
// see ui/hallow.css). Everything else is one click away in Customize.
pref("browser.uiCustomization.state", "{\"placements\":{\"nav-bar\":[\"back-button\",\"forward-button\",\"stop-reload-button\",\"urlbar-container\",\"downloads-button\"],\"TabsToolbar\":[\"tabbrowser-tabs\",\"new-tab-button\",\"alltabs-button\"]},\"seen\":[],\"dirtyAreaCache\":[\"nav-bar\",\"TabsToolbar\"],\"currentVersion\":26,\"newElementCount\":0}");

// No bookmarks bar popping in on the new tab page.
pref("browser.toolbars.bookmarks.visibility", "never");
// Show example.com instead of https://example.com.
pref("browser.urlbar.trimHttps", true);
pref("identity.fxaccounts.toolbar.enabled", false);

// New tab: search and your own shortcuts, nothing else.
pref("browser.newtabpage.activity-stream.feeds.section.topstories", false);
pref("browser.newtabpage.activity-stream.showSponsored", false);
pref("browser.newtabpage.activity-stream.showSponsoredTopSites", false);
pref("browser.newtabpage.activity-stream.showSponsoredCheckboxes", false);
pref("browser.newtabpage.activity-stream.system.showSponsored", false);
pref("browser.newtabpage.activity-stream.showWeather", false);
pref("browser.newtabpage.activity-stream.default.sites", "");
// Use that empty list instead of the preset shortcuts (Wikipedia, YouTube,
// ...) Mozilla serves through Remote Settings.
pref("browser.topsites.useRemoteSetting", false);
pref("browser.topsites.contile.enabled", false);

// Address bar: no sponsored, trending or partner suggestions.
pref("browser.urlbar.quicksuggest.enabled", false);
pref("browser.urlbar.suggest.quicksuggest.sponsored", false);
pref("browser.urlbar.suggest.quicksuggest.all", false);
pref("browser.urlbar.trending.featureGate", false);
pref("browser.urlbar.weather.featureGate", false);
pref("browser.urlbar.addons.featureGate", false);
pref("browser.urlbar.mdn.featureGate", false);
pref("browser.urlbar.yelp.featureGate", false);

// No onboarding tours, promos, recommendations or "what's new" pages.
pref("browser.aboutwelcome.enabled", false);
pref("browser.startup.homepage_override.mstone", "ignore");
pref("browser.shell.checkDefaultBrowser", false);
pref("browser.uitour.enabled", false);
pref("browser.preferences.moreFromMozilla", false);
pref("browser.vpn_promo.enabled", false);
pref("browser.promo.pin.enabled", false);
pref("browser.discovery.enabled", false);
pref("browser.newtabpage.activity-stream.asrouter.userprefs.cfr.addons", false);
pref("browser.newtabpage.activity-stream.asrouter.userprefs.cfr.features", false);
pref("extensions.htmlaboutaddons.recommendations.enabled", false);
pref("extensions.getAddons.showPane", false);

// ---- Lightweight -------------------------------------------------------------

// AI features (chatbot sidebar, smart tab groups, link previews, smart
// window) are blocked by default through Firefox's own AI controls, so their
// models are never downloaded. Local, private translation stays available.
// Both can be changed in Settings > AI Controls.
pref("browser.ai.control.default", "blocked");
pref("browser.ai.control.translations", "enabled");
// What each feature's own "Block" action in Settings turns off, so blocked
// features are also hidden (e.g. "Ask an AI Chatbot" in the context menu).
pref("browser.ml.chat.enabled", false);
pref("browser.ml.chat.page", false);
pref("browser.ml.linkPreview.enabled", false);
pref("browser.tabs.groups.smart.enabled", false);
pref("browser.tabs.groups.smart.userEnabled", false);
pref("pdfjs.enableGuessAltText", false);
pref("pdfjs.enableAltTextModelDownload", false);

// Let Firefox unload long-inactive background tabs when the system is
// actually under memory pressure (Firefox's Linux default is never). Nothing
// is unloaded while memory is plentiful: the trigger is the memory watcher's
// low-memory signal, and only tabs idle for 10+ minutes are candidates.
pref("browser.tabs.unloadOnLowMemory", true);
// Batch session-restore writes: every 60 s instead of every 15 s, a quarter
// of the disk writes for the same crash protection of open tabs.
pref("browser.sessionstore.interval", 60000);

// ---- Speed -------------------------------------------------------------------
// Hallow's speed comes from the build (profile-guided and cross-language
// link-time optimization, see mozconfig), not from pref tweaks: networking,
// HTTP/2 and HTTP/3, DNS prefetch and preconnect, caches, WebRender, the
// JavaScript and WebAssembly JITs all run with Firefox's tuned defaults.

// Video: steer streaming sites (Media Source Extensions) towards a codec the
// GPU decodes, best first: AV1, then VP9, then H.264. AV1 is only offered
// where it decodes in hardware (in software it is the most CPU-hungry), and
// VP9 is withheld where it would decode in software while H.264 decodes in
// hardware. Hardware decoding itself (VA-API on Linux) and its fallback to
// software stay under Firefox's own driver checks. See patches/0006.
pref("media.mediasource.prefer-hardware-codecs", true);

// Built-in add-ons (new tab page, web compatibility fixes) are the versions
// shipped with each Hallow release; Mozilla's system add-on update service
// only serves Firefox, so asking it daily is pointless background traffic.
// Web compatibility interventions themselves stay fully enabled.
pref("extensions.systemAddon.update.enabled", false);

// ---- Updates ------------------------------------------------------------------
// Hallow updates from its own stable channel, which only release-branch
// builds reach, never from Mozilla's update servers: Linux packages through
// the system package manager (LinuxPackageUpdater), checked once a day by
// Firefox's update timer (app.update.interval) and in About Hallow.
// Where to go when an update cannot be installed automatically:
pref("app.update.url.manual", "https://github.com/YAD-ctrlz/Hallow-browser/releases/latest");
pref("app.update.url.details", "https://github.com/YAD-ctrlz/Hallow-browser/releases");

// ---- Privacy: no telemetry or studies ------------------------------------------

// Hallow is built without MOZILLA_OFFICIAL, so telemetry upload is not even
// compiled in. These prefs keep the remaining reporting paths and their UI off.
pref("datareporting.policy.dataSubmissionEnabled", false);
pref("datareporting.policy.firstRunURL", "");
pref("datareporting.healthreport.uploadEnabled", false);
pref("datareporting.usage.uploadEnabled", false);
pref("toolkit.telemetry.enabled", false);
pref("toolkit.telemetry.unified", false);
pref("toolkit.telemetry.archive.enabled", false);
pref("toolkit.telemetry.newProfilePing.enabled", false);
pref("toolkit.telemetry.shutdownPingSender.enabled", false);
pref("toolkit.telemetry.updatePing.enabled", false);
pref("toolkit.telemetry.bhrPing.enabled", false);
pref("toolkit.telemetry.firstShutdownPing.enabled", false);
pref("toolkit.coverage.opt-out", true);
pref("app.shield.optoutstudies.enabled", false);
pref("app.normandy.enabled", false);
pref("app.normandy.api_url", "");
pref("browser.newtabpage.activity-stream.feeds.telemetry", false);
pref("browser.newtabpage.activity-stream.telemetry", false);
pref("browser.tabs.crashReporting.sendReport", false);
pref("browser.crashReports.unsubmittedCheck.autoSubmit2", false);
// Ask websites not to sell or share your data.
pref("privacy.globalprivacycontrol.enabled", true);
