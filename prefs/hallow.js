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

// Unload background tabs when the system runs low on memory (Firefox only
// does this on Windows and macOS by default).
pref("browser.tabs.unloadOnLowMemory", true);
// Write the session to disk every 60s instead of every 15s.
pref("browser.sessionstore.interval", 60000);

// ---- Speed -------------------------------------------------------------------
// (The biggest speed-up is in the build: profile-guided + link-time
// optimization, see mozconfig.) These raise limits Firefox sized for slower
// networks and machines.

// Networking: more parallel connections, no artificial request pacing, and
// longer-lived DNS and TLS session caches for faster repeat visits.
pref("network.http.max-connections", 1800);
pref("network.http.max-persistent-connections-per-server", 10);
pref("network.http.max-urgent-start-excessive-connections-per-host", 5);
pref("network.http.pacing.requests.enabled", false);
pref("network.dnsCacheExpiration", 3600);
pref("network.ssl_tokens_cache_capacity", 16384);
// Rendering: paint pages that are still loading sooner (120 ms -> 100 ms),
// and bigger caches for accelerated canvas, glyphs and image decoding.
pref("content.notify.interval", 100000);
pref("gfx.canvas.accelerated.cache-items", 32768);
pref("gfx.canvas.accelerated.cache-size", 512);
pref("gfx.content.skia-font-cache-size", 32);
pref("image.mem.decode_bytes_at_a_time", 32768);
// Media: buffer further ahead so playback does not stall on busy networks.
pref("media.memory_cache_max_size", 65536);
pref("media.cache_readahead_limit", 600);
pref("media.cache_resume_threshold", 300);
// Built-in add-ons (new tab page, web compatibility fixes) ship with each
// Hallow release instead of being swapped out by Mozilla's update channel,
// which would also drop Hallow's new tab styling.
pref("extensions.systemAddon.update.enabled", false);

// ---- Rust-first engine features ------------------------------------------------

// WebGPU through wgpu, Mozilla's Rust graphics stack. Enabled in Firefox
// Nightly and Beta on Linux; Release still has it off.
pref("dom.webgpu.enabled", true);
// JPEG XL decoding through jxl-rs, the Rust JPEG XL decoder. Nightly-only
// in Firefox.
pref("image.jxl.enabled", true);

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
