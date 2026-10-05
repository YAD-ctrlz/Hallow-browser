"""Minimal client for Marionette, Gecko's remote control protocol.

Only what Hallow's tests need (a session, chrome context, async scripts),
so the tests do not depend on Mozilla's Python test packages.
"""

import json
import socket
import time


class Marionette:
    def __init__(self, host="127.0.0.1", port=2828, timeout=90):
        deadline = time.time() + timeout
        while True:
            try:
                self.sock = socket.create_connection((host, port), timeout=10)
                break
            except OSError:
                if time.time() > deadline:
                    raise
                time.sleep(0.5)
        self.sock.settimeout(900)
        self.buf = b""
        self.msgid = 0
        self._read()  # the server's hello
        self.call("WebDriver:NewSession", {"capabilities": {}})

    def _recv(self):
        data = self.sock.recv(65536)
        if not data:
            raise ConnectionError("Marionette closed the connection")
        return data

    def _read(self):
        while b":" not in self.buf:
            self.buf += self._recv()
        length, rest = self.buf.split(b":", 1)
        length = int(length)
        while len(rest) < length:
            rest += self._recv()
        self.buf = rest[length:]
        return json.loads(rest[:length])

    def call(self, name, params=None):
        self.msgid += 1
        message = json.dumps([0, self.msgid, name, params or {}]).encode()
        self.sock.sendall(str(len(message)).encode() + b":" + message)
        while True:
            reply = self._read()
            if reply[0] == 1 and reply[1] == self.msgid:
                if reply[2]:
                    raise RuntimeError(f"{name}: {reply[2]}")
                return reply[3]

    def chrome(self):
        self.call("Marionette:SetContext", {"value": "chrome"})

    def content(self):
        self.call("Marionette:SetContext", {"value": "content"})

    def navigate(self, url):
        """Load `url` in the current tab (switches to the content context)."""
        self.content()
        self.call("WebDriver:Navigate", {"url": url})

    def run(self, script, args=None, timeout_ms=600000):
        """Run `script` as an async function in the current context; its
        return value is returned. `args` are available as `args`."""
        self.call("WebDriver:SetTimeouts", {"script": timeout_ms})
        body = (
            "const done = arguments[arguments.length - 1];"
            "const args = Array.from(arguments).slice(0, -1);"
            "(async () => {" + script + "})().then(done, e => done("
            "{__error: String(e) + '\\n' + (e && e.stack)}));"
        )
        value = self.call(
            "WebDriver:ExecuteAsyncScript", {"script": body, "args": args or []}
        )["value"]
        if isinstance(value, dict) and "__error" in value:
            raise RuntimeError(value["__error"])
        return value

    def quit(self):
        try:
            self.call("Marionette:Quit", {"flags": ["eForceQuit"]})
        except (ConnectionError, OSError, RuntimeError):
            pass
