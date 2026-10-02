/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Updates for Hallow installed from its Linux package (.deb).
 *
 * The files of a package installation belong to the system package manager.
 * Gecko's own updater can neither write them (they are root-owned) nor
 * should it (dpkg would no longer know what is installed), which is why
 * Firefox hides its update UI in packaged apps (sysinfo "isPackagedApp").
 * Hallow keeps that UI and installs updates through the package manager:
 *
 * - check() asks APT, unprivileged and with a private list cache, about
 *   Hallow's stable channel only (/etc/apt/sources.list.d/hallow.sources).
 *   APT accepts the channel's metadata only if it is signed with the Hallow
 *   archive key, so "a new version is available" comes from verified data.
 * - install() runs hallow-update-helper through pkexec: the system asks for
 *   an administrator password and the helper upgrades the hallow package
 *   with APT, which checks every file against the signed metadata and never
 *   downgrades. The running Hallow keeps working; a restart switches to the
 *   new version.
 * - Firefox's update timer manager calls LinuxPackageUpdateTimer once per
 *   app.update.interval (LinuxPackageUpdater.manifest). Nothing runs at
 *   startup; an available update is offered with Firefox's own update
 *   notifications.
 *
 * AppUpdater (About dialog, Settings) uses check() and install().
 */

import { AppConstants } from "resource://gre/modules/AppConstants.sys.mjs";

const lazy = {};

ChromeUtils.defineESModuleGetters(lazy, {
  FileUtils: "resource://gre/modules/FileUtils.sys.mjs",
  Subprocess: "resource://gre/modules/Subprocess.sys.mjs",
  UpdateListener: "resource://gre/modules/UpdateListener.sys.mjs",
  UpdateLog: "resource://gre/modules/UpdateLog.sys.mjs",
});

const PACKAGE = "hallow";
const SOURCES = "/etc/apt/sources.list.d/hallow.sources";
const APT_GET = "/usr/bin/apt-get";
const APT_CACHE = "/usr/bin/apt-cache";
const DPKG_QUERY = "/usr/bin/dpkg-query";
const PKEXEC = "/usr/bin/pkexec";
const HELPER = "hallow-update-helper";

// pkexec's exit codes when the user dismissed the authentication dialog, and
// when authorization failed (wrong password, no authentication agent).
const PKEXEC_DISMISSED = 126;
const PKEXEC_NOT_AUTHORIZED = 127;

const PREF_BACKGROUND_ERRORS = "app.update.backgroundErrors";
const PREF_BACKGROUND_MAX_ERRORS = "app.update.backgroundMaxErrors";

function LOG(string) {
  lazy.UpdateLog.logPrefixedString("AUS:PKG", string);
}

export class PackageUpdateError extends Error {
  constructor(message) {
    super(message);
    this.name = "PackageUpdateError";
  }
}

/**
 * The last lines of a command's output, for error messages.
 */
function tail(output, lines = 6) {
  return output.trim().split("\n").slice(-lines).join("\n");
}

/**
 * Runs `command` without a shell and collects its output.
 *
 * @returns {Promise<{exitCode: number, output: string}>}
 */
async function run(command, args) {
  let proc;
  try {
    proc = await lazy.Subprocess.call({
      command,
      arguments: args,
      // Untranslated output, which parsePolicy() relies on.
      environment: { LC_ALL: "C", LANGUAGE: null },
      environmentAppend: true,
      stderr: "stdout",
    });
  } catch (e) {
    throw new PackageUpdateError(`Could not run ${command}: ${e.message}`);
  }
  let output = "";
  let chunk;
  while ((chunk = await proc.stdout.readString())) {
    output += chunk;
  }
  const { exitCode } = await proc.wait();
  LOG(`run - ${command} ${args.join(" ")} exited with ${exitCode}`);
  return { exitCode, output };
}

/**
 * Reads the installed and candidate versions from `apt-cache policy`.
 *
 * @param {string} output
 * @returns {{installed: string?, candidate: string?}}
 */
export function parsePolicy(output) {
  const field = name => {
    const value = output.match(new RegExp(`^\\s*${name}:\\s*(\\S+)\\s*$`, "m"));
    return value && value[1] != "(none)" ? value[1] : null;
  };
  return { installed: field("Installed"), candidate: field("Candidate") };
}

class PackageUpdater {
  #installing = null;
  #availableVersion = null;

  get helperPath() {
    return PathUtils.join(Services.dirsvc.get("GreD", Ci.nsIFile).path, HELPER);
  }

  /**
   * Whether this Hallow is updated through its package: it was installed
   * from the .deb, which brings the update helper and Hallow's APT source
   * (deleting /etc/apt/sources.list.d/hallow.sources opts out).
   */
  get available() {
    if (!Services.sysinfo.getProperty("isPackagedApp")) {
      return false;
    }
    const exists = path => {
      try {
        return new lazy.FileUtils.File(path).exists();
      } catch (e) {
        return false;
      }
    };
    return [SOURCES, this.helperPath, PKEXEC, APT_GET, APT_CACHE].every(
      exists
    );
  }

  /**
   * The version this Hallow runs. It is the package version: the build
   * writes it to version_display.txt.
   */
  get runningVersion() {
    return AppConstants.MOZ_APP_VERSION_DISPLAY;
  }

  /**
   * APT options that limit it to Hallow's channel and keep its lists in the
   * profile's cache directory, so checking needs no root and never touches
   * the system's package lists.
   */
  async aptOptions() {
    const dir = PathUtils.join(PathUtils.localProfileDir, "updates", "apt");
    await IOUtils.makeDirectory(PathUtils.join(dir, "lists", "partial"));
    await IOUtils.makeDirectory(PathUtils.join(dir, "archives", "partial"));
    return [
      "-o",
      `Dir::Etc::SourceList=${SOURCES}`,
      "-o",
      "Dir::Etc::SourceParts=-",
      "-o",
      `Dir::State::Lists=${PathUtils.join(dir, "lists")}`,
      "-o",
      `Dir::Cache=${dir}`,
      "-o",
      "Debug::NoLocking=true",
    ];
  }

  /**
   * The hallow package version dpkg has installed.
   */
  async installedVersion() {
    const result = await run(DPKG_QUERY, ["-W", "-f=${Version}", PACKAGE]);
    return result.exitCode == 0 ? result.output.trim() || null : null;
  }

  /**
   * Checks Hallow's stable channel.
   *
   * @returns {Promise<{status: string, version: string}>}
   *   status "available": `version` can be installed;
   *   status "restart": `version` is already installed (for example by the
   *     system's update manager) and runs after a restart;
   *   status "up-to-date".
   * @throws {PackageUpdateError} if the channel could not be checked, for
   *   example offline or when its metadata fails verification.
   */
  async check() {
    const options = await this.aptOptions();
    let result = await run(APT_GET, ["-q", "update", ...options]);
    if (result.exitCode != 0) {
      throw new PackageUpdateError(
        `Checking Hallow's update channel failed:\n${tail(result.output)}`
      );
    }
    result = await run(APT_CACHE, [...options, "policy", PACKAGE]);
    if (result.exitCode != 0) {
      throw new PackageUpdateError(
        `Reading Hallow's update channel failed:\n${tail(result.output)}`
      );
    }
    const { installed, candidate } = parsePolicy(result.output);
    LOG(
      `check - running ${this.runningVersion}, installed ${installed}, ` +
        `channel offers ${candidate}`
    );
    if (!installed) {
      throw new PackageUpdateError(`The ${PACKAGE} package is not installed`);
    }
    // Without pinning, APT's candidate is never older than the installed
    // version: it does not downgrade.
    if (candidate && candidate != installed) {
      this.#availableVersion = candidate;
      return { status: "available", version: candidate };
    }
    if (installed != this.runningVersion) {
      return { status: "restart", version: installed };
    }
    return { status: "up-to-date", version: installed };
  }

  /**
   * Installs the newest version from Hallow's channel. The system asks for
   * an administrator password first.
   *
   * @returns {Promise<string>} "installed" (restart to finish),
   *   "cancelled" (the password prompt was dismissed) or "up-to-date".
   * @throws {PackageUpdateError} if installing failed.
   */
  install() {
    // Several windows may ask at once; run the package manager once.
    if (!this.#installing) {
      this.#installing = this.#install().finally(() => {
        this.#installing = null;
      });
    }
    return this.#installing;
  }

  async #install() {
    LOG(`install - running ${this.helperPath} through pkexec`);
    const result = await run(PKEXEC, [this.helperPath, "upgrade"]);
    if (result.exitCode == PKEXEC_DISMISSED) {
      return "cancelled";
    }
    if (result.exitCode == PKEXEC_NOT_AUTHORIZED) {
      throw new PackageUpdateError(
        `Not authorized to install the update:\n${tail(result.output)}`
      );
    }
    if (result.exitCode != 0) {
      throw new PackageUpdateError(
        `Installing the update failed:\n${tail(result.output)}`
      );
    }
    const installed = await this.installedVersion();
    LOG(`install - installed version is now ${installed}`);
    if (!installed || installed == this.runningVersion) {
      return "up-to-date";
    }
    // Keep a restart reminder in the app menu, wherever the install started.
    this.#showRestart(true);
    return "installed";
  }

  /**
   * The daily background check (LinuxPackageUpdateTimer). Offers an
   * available update with Firefox's update notification; installing it
   * still needs the user's go-ahead and an administrator password.
   */
  async backgroundCheck() {
    if (
      !this.available ||
      (Services.policies && !Services.policies.isAllowed("appUpdate"))
    ) {
      return;
    }
    let result;
    try {
      result = await this.check();
    } catch (e) {
      // Offline or a broken channel: retry at the next interval, and tell
      // the user once checks have kept failing for a while.
      const errors =
        Services.prefs.getIntPref(PREF_BACKGROUND_ERRORS, 0) + 1;
      Services.prefs.setIntPref(PREF_BACKGROUND_ERRORS, errors);
      LOG(`backgroundCheck - check ${errors} failed: ${e.message}`);
      if (errors >= Services.prefs.getIntPref(PREF_BACKGROUND_MAX_ERRORS, 10)) {
        this.#showFailure(true);
      }
      return;
    }
    Services.prefs.clearUserPref(PREF_BACKGROUND_ERRORS);
    if (result.status == "available") {
      this.#showAvailable(false);
    } else if (result.status == "restart") {
      this.#showRestart(true);
    }
  }

  #showAvailable(dismissed) {
    lazy.UpdateListener.showUpdateNotification(
      "available",
      () => this.#installFromNotification(),
      false,
      { dismissed }
    );
  }

  #showRestart(dismissed) {
    lazy.UpdateListener.showUpdateNotification(
      "restart",
      () => lazy.UpdateListener.requestRestart(),
      true,
      { dismissed }
    );
  }

  #showFailure(dismissed) {
    // Firefox's "can't update" notification; it links to the downloads page
    // (app.update.url.manual) as the way out.
    lazy.UpdateListener.showUpdateNotification(
      "manual",
      win => lazy.UpdateListener.openManualUpdateUrl(win),
      false,
      { dismissed }
    );
  }

  async #installFromNotification() {
    lazy.UpdateListener.clearPendingAndActiveNotifications();
    let outcome;
    try {
      outcome = await this.install();
    } catch (e) {
      console.error(e);
      this.#showFailure(false);
      return;
    }
    if (outcome == "installed") {
      this.#showRestart(false);
    } else if (outcome == "cancelled" && this.#availableVersion) {
      this.#showAvailable(true);
    }
  }
}

export const LinuxPackageUpdater = new PackageUpdater();

/**
 * Called by Firefox's update timer manager; see LinuxPackageUpdater.manifest.
 */
export class LinuxPackageUpdateTimer {
  notify() {
    LinuxPackageUpdater.backgroundCheck().catch(e => console.error(e));
  }
}

LinuxPackageUpdateTimer.prototype.classID = Components.ID(
  "{9b0c5a8e-6f7d-4c3e-9d2a-5e1f4b8c7a61}"
);
LinuxPackageUpdateTimer.prototype.QueryInterface = ChromeUtils.generateQI([
  "nsITimerCallback",
]);
