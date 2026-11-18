// PANIM puts this script in the main frame of each account window, before the scripts of the page.
// Refer to `docs/ARCHITECTURE.md` §5.
//
// The script replaces `window.Notification`. When the page makes a notification, the script sends one signal
// to PANIM. The signal has no content: the script does not send the title, the body or other data of the
// notification. The script does not read the page. The Rust side reads the unread count from the title of the page.
(function () {
  "use strict";

  // Keep the Tauri function now, so that a later change by the page has no effect on it.
  var internals = window.__TAURI_INTERNALS__;
  var invoke = internals && internals.invoke;

  // Send the signal. The command has no arguments. Ignore all errors.
  function signal() {
    try {
      if (typeof invoke === "function") {
        invoke.call(internals, "page_notice", {}).catch(function () {});
      }
    } catch (e) {
      // Ignore the error.
    }
  }

  function noop() {}

  // The replacement. It ignores its arguments and gives an object that does nothing.
  function Notice() {
    signal();
    if (!(this instanceof Notice)) {
      return;
    }
    this.title = "";
    this.body = "";
    this.tag = "";
    this.icon = "";
    this.data = null;
    this.onclick = null;
    this.onshow = null;
    this.onerror = null;
    this.onclose = null;
  }
  Notice.prototype.close = noop;
  Notice.prototype.addEventListener = noop;
  Notice.prototype.removeEventListener = noop;
  Notice.prototype.dispatchEvent = function () {
    return true;
  };

  // The page sees that it has the permission. It does not ask the person.
  Object.defineProperty(Notice, "permission", {
    get: function () {
      return "granted";
    }
  });
  Notice.maxActions = 0;
  Notice.requestPermission = function (callback) {
    if (typeof callback === "function") {
      setTimeout(function () {
        try {
          callback("granted");
        } catch (e) {
          // Ignore the error.
        }
      }, 0);
    }
    return Promise.resolve("granted");
  };

  try {
    Object.defineProperty(window, "Notification", {
      value: Notice,
      writable: true,
      configurable: true
    });
  } catch (e) {
    // Ignore the error.
  }
})();
