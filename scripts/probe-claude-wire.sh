#!/usr/bin/env bash
# Probe Claude Code's stream-json wire, the way the agent host drives it: one process,
# many turns, permissions answered on stdin, control requests, a kill and a resume.
# Every line in and out lands in <out-dir>/<n>-<scenario>.jsonl with its direction and
# time, so the lines can be read back as test fixtures after scrubbing paths.
#
#   scripts/probe-claude-wire.sh <out-dir> [scenario ...]
#
# Runs every scenario when none is named. Costs a few haiku turns on the box's own
# Claude Code login; nothing else leaves the machine. Never run it in a real project:
# it writes files in a scratch directory it creates under <out-dir>/cwd.
set -euo pipefail

out="${1:?usage: probe-claude-wire.sh <out-dir> [scenario ...]}"
shift
mkdir -p "$out/cwd"
export PROBE_OUT="$out"
export PROBE_CLAUDE="${RUSTY_CLAUDE_BIN:-claude}"
export PROBE_MCP_URL="${RUSTY_MCP_URL:-http://127.0.0.1:4174/mcp}"

python3 - "$@" <<'PY'
import json, os, queue, signal, subprocess, sys, threading, time, uuid

OUT = os.environ["PROBE_OUT"]
CLAUDE = os.environ["PROBE_CLAUDE"]
MCP_URL = os.environ["PROBE_MCP_URL"]
CWD = os.path.join(OUT, "cwd")
BASE = ["-p", "--input-format", "stream-json", "--output-format", "stream-json",
        "--include-partial-messages", "--verbose", "--permission-prompt-tool", "stdio",
        "--model", "haiku"]
ENV = {k: v for k, v in os.environ.items() if not k.startswith(("CLAUDECODE", "CLAUDE_CODE_", "CLAUDE_PID", "CLAUDE_EFFORT"))}


def user(text):
    return {"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}}


def control(request_id, request):
    return {"type": "control_request", "request_id": request_id, "request": request}


def response(request_id, body):
    return {"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": body}}


class Run:
    def __init__(self, name, args, cwd=CWD):
        self.path = os.path.join(OUT, name + ".jsonl")
        self.f = open(self.path, "a")
        self.t0 = time.time()
        self.lines = queue.Queue()
        cmd = [CLAUDE] + BASE + args
        self.record("cmd", " ".join(cmd))
        self.p = subprocess.Popen(cmd, cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True, bufsize=1, env=ENV)
        threading.Thread(target=self._read, daemon=True).start()
        threading.Thread(target=self._err, daemon=True).start()

    def _read(self):
        for line in self.p.stdout:
            obj = self.record("out", line)
            self.lines.put(obj)
        self.lines.put(None)

    def _err(self):
        for line in self.p.stderr:
            self.record("err", line)

    def record(self, direction, line):
        text = line.rstrip("\n")
        try:
            obj = json.loads(text)
        except ValueError:
            obj = text
        self.f.write(json.dumps({"dir": direction, "t": round(time.time() - self.t0, 3), "line": obj}) + "\n")
        self.f.flush()
        return obj

    def send(self, obj):
        s = json.dumps(obj)
        self.record("in", s)
        self.p.stdin.write(s + "\n")
        self.p.stdin.flush()

    def note(self, text):
        self.record("note", text)
        print(f"  {text}")

    def wait_for(self, pred, timeout=90):
        end = time.time() + timeout
        while time.time() < end:
            try:
                obj = self.lines.get(timeout=max(0.05, end - time.time()))
            except queue.Empty:
                break
            if obj is None:
                self.note("stdout closed")
                return None
            if isinstance(obj, dict) and pred(obj):
                return obj
        self.note("timed out waiting")
        return None

    def wait_result(self, timeout=120):
        return self.wait_for(lambda o: o.get("type") == "result", timeout)

    def wait_permission(self, timeout=90):
        return self.wait_for(lambda o: o.get("type") == "control_request" and o.get("request", {}).get("subtype") == "can_use_tool", timeout)

    def rss(self):
        try:
            with open(f"/proc/{self.p.pid}/status") as s:
                for l in s:
                    if l.startswith("VmRSS"):
                        return l.split()[1] + " kB"
        except OSError:
            pass
        return "?"

    def close(self, timeout=30):
        try:
            self.p.stdin.close()
        except OSError:
            pass
        try:
            code = self.p.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.p.kill()
            code = self.p.wait()
            self.note("killed after the close timeout")
        self.note(f"exit {code}")
        self.f.close()
        return code


def allow(run, req):
    run.send(response(req["request_id"], {"behavior": "allow", "updatedInput": req["request"].get("input", {})}))


def s_turns():
    sid = str(uuid.uuid4())
    r = Run("01-turns", ["--session-id", sid, "--name", "probe-turns", "--replay-user-messages"])
    r.wait_for(lambda o: o.get("type") == "system" and o.get("subtype") == "init", 60)
    r.note(f"rss after init {r.rss()}")
    r.send(user("Reply with the single word ping."))
    r.wait_result()
    r.note(f"rss after a turn {r.rss()}")
    r.send(user("Reply with the single word pong."))
    r.wait_result()
    time.sleep(1)
    r.close()


def s_write_allow():
    r = Run("02-write-allow", ["--session-id", str(uuid.uuid4())])
    r.send(user("Use the Write tool to create a file named hello.txt in the current directory containing exactly the word hello. Do not read anything first."))
    req = r.wait_permission()
    if req:
        allow(r, req)
        r.wait_result()
    r.close()


def s_write_deny():
    r = Run("03-write-deny", ["--session-id", str(uuid.uuid4())])
    r.send(user("Use the Write tool to create a file named denied.txt in the current directory containing exactly the word hello. Do not read anything first."))
    req = r.wait_permission()
    if req:
        r.send(response(req["request_id"], {"behavior": "deny", "message": "Not that file. Say in one line why you wanted it and stop."}))
        r.wait_result()
    r.close()


def s_interrupt():
    r = Run("04-interrupt", ["--session-id", str(uuid.uuid4())])
    r.send(user("Count from 1 to 400, one number per line, with no tools and no commentary."))
    r.wait_for(lambda o: o.get("type") == "stream_event" and o["event"].get("delta", {}).get("type") == "text_delta", 60)
    time.sleep(1.5)
    r.send(control("rusty-interrupt-1", {"subtype": "interrupt"}))
    r.wait_result(30)
    time.sleep(1)
    r.close()


def s_mode():
    r = Run("05-mode", ["--session-id", str(uuid.uuid4()), "--permission-mode", "default"])
    r.wait_for(lambda o: o.get("type") == "system" and o.get("subtype") == "init", 60)
    r.send(control("rusty-mode-1", {"subtype": "set_permission_mode", "mode": "acceptEdits"}))
    r.wait_for(lambda o: o.get("type") == "control_response", 10)
    r.send(user("Use the Write tool to create a file named accepted.txt in the current directory containing exactly the word hello. Do not read anything first."))
    got = r.wait_for(lambda o: o.get("type") in ("result",) or (o.get("type") == "control_request" and o["request"].get("subtype") == "can_use_tool"), 90)
    if got and got.get("type") == "control_request":
        r.note("acceptEdits still prompted")
        allow(r, got)
        r.wait_result()
    r.send(control("rusty-mode-2", {"subtype": "set_permission_mode", "mode": "default"}))
    r.wait_for(lambda o: o.get("type") == "control_response", 10)
    r.send(user("Now use the Write tool to create a file named second.txt containing exactly the word hello."))
    req = r.wait_permission()
    if req:
        allow(r, req)
        r.wait_result()
    for mode in ("manual", "plan", "bogus"):
        r.send(control(f"rusty-mode-{mode}", {"subtype": "set_permission_mode", "mode": mode}))
        r.wait_for(lambda o: o.get("type") == "control_response", 10)
    r.close()


def s_model():
    r = Run("06-model", ["--session-id", str(uuid.uuid4())])
    r.wait_for(lambda o: o.get("type") == "system" and o.get("subtype") == "init", 60)
    r.send(control("rusty-model-1", {"subtype": "set_model", "model": "sonnet"}))
    r.wait_for(lambda o: o.get("type") == "control_response", 10)
    r.send(user("Reply with the single word ok."))
    r.wait_result()
    r.send(control("rusty-model-2", {"subtype": "set_model", "model": "haiku"}))
    r.wait_for(lambda o: o.get("type") == "control_response", 10)
    r.send(user("Reply with the single word ok."))
    r.wait_result()
    r.close()


def s_sigterm_resume():
    sid = str(uuid.uuid4())
    a = Run("07a-sigterm", ["--session-id", sid])
    a.send(user("Count from 1 to 400, one number per line, with no tools and no commentary."))
    for _ in range(3):
        a.wait_for(lambda o: o.get("type") == "stream_event" and o["event"].get("delta", {}).get("type") == "text_delta", 60)
    a.p.send_signal(signal.SIGTERM)
    a.note("sent SIGTERM")
    a.wait_for(lambda o: False, 5)
    a.close(20)
    b = Run("07b-resume", ["--resume", sid])
    b.wait_for(lambda o: o.get("type") == "system" and o.get("subtype") == "init", 60)
    got = b.wait_for(lambda o: o.get("type") == "result", 8)
    b.note("continued on its own" if got else "waited 8 s after init: no turn on its own")
    b.send(user("What was the last number you wrote? Answer with the number only."))
    b.wait_result()
    b.close()


def s_session_id():
    sid = str(uuid.uuid4())
    a = Run("08a-session-id", ["--session-id", sid])
    a.send(user("Reply with the single word one."))
    a.wait_result()
    a.close()
    b = Run("08b-resume", ["--resume", sid])
    b.send(user("Which single word did you reply with before? Answer with that word only."))
    b.wait_result()
    b.close()
    c = Run("08c-session-id-reused", ["--session-id", sid])
    c.send(user("Reply with the single word three."))
    c.wait_result(60)
    c.close()


def s_mcp_clash():
    cwd = os.path.join(CWD, "clash")
    os.makedirs(cwd, exist_ok=True)
    with open(os.path.join(cwd, ".mcp.json"), "w") as f:
        json.dump({"mcpServers": {"rusty": {"type": "stdio", "command": "rusty-mcp"}}}, f)
    cfg = json.dumps({"mcpServers": {"rusty": {"type": "http", "url": MCP_URL}}})
    for name, extra in (("09a-mcp-clash", []), ("09b-mcp-clash-strict", ["--strict-mcp-config"])):
        r = Run(name, ["--session-id", str(uuid.uuid4()), "--mcp-config", cfg] + extra, cwd=cwd)
        init = r.wait_for(lambda o: o.get("type") == "system" and o.get("subtype") == "init", 60)
        if init:
            r.note("mcp_servers " + json.dumps(init.get("mcp_servers")) + " errors " + json.dumps(init.get("mcp_server_errors")))
        r.send(user("Reply with the single word ok."))
        r.wait_result()
        r.close()


def s_queue():
    r = Run("10-queue", ["--session-id", str(uuid.uuid4())])
    r.send(user("Reply with the single word alpha."))
    r.send(user("Reply with the single word beta."))
    n = 0
    while r.wait_result(60):
        n += 1
        if n == 2:
            break
    r.note(f"results seen {n}")
    r.close()


def s_thinking():
    r = Run("11-thinking", ["--session-id", str(uuid.uuid4())])
    r.send(user("Think it through step by step before answering: a bat and a ball cost 1.10 in total, the bat costs 1.00 more than the ball, what does the ball cost? Answer with the amount only."))
    r.wait_result()
    r.close()


SCENARIOS = [("turns", s_turns), ("write-allow", s_write_allow), ("write-deny", s_write_deny),
             ("interrupt", s_interrupt), ("mode", s_mode), ("model", s_model),
             ("sigterm-resume", s_sigterm_resume), ("session-id", s_session_id),
             ("mcp-clash", s_mcp_clash), ("queue", s_queue), ("thinking", s_thinking)]

wanted = sys.argv[1:]
for name, fn in SCENARIOS:
    if wanted and name not in wanted:
        continue
    print(f"== {name}")
    try:
        fn()
    except Exception as e:  # keep going: one broken scenario must not hide the rest
        print(f"  failed: {e}")
print(f"lines under {OUT}")
PY
