#!/usr/bin/env python3
"""
Testa o fluxo DAP completo com codelldb via Python.

Fluxo correto (per DAP spec):
  1. initialize
  2. launch  (sem esperar response)
  3. Esperar evento "initialized"  <- adapter pronto para config
  4. setBreakpoints
  5. setExceptionBreakpoints
  6. configurationDone
  7. Esperar launch response
  8. Esperar stopped(entry)
  9. continue -> esperar stopped(breakpoint)
  10. stackTrace -> continue -> terminated

Uso:
  python scripts/test_dap_flow.py [<path-to-exe>]
"""

import json
import subprocess
import sys
import threading
import time
from pathlib import Path

CODELLDB = Path(r"C:\workspace\rdc_app\tools\codelldb\extension\adapter\codelldb.exe")
SRC_FILE = Path(r"C:\workspace\rdc_app\crates\debug-target-example\src\main.rs")
BREAKPOINT_LINE = 20  # let mut swapped = false; — primeira linha executavel em bubble_sort


def find_codelldb() -> Path:
    if CODELLDB.exists():
        return CODELLDB
    raise FileNotFoundError("codelldb nao encontrado em " + str(CODELLDB))


class DapSession:
    def __init__(self, proc: subprocess.Popen):
        self.proc = proc
        self.seq = 1
        self._read_buf = b""

    def send(self, command: str, arguments: dict | None = None) -> int:
        msg = {"seq": self.seq, "type": "request", "command": command}
        if arguments is not None:
            msg["arguments"] = arguments
        payload = json.dumps(msg, separators=(",", ":")).encode("utf-8")
        header = f"Content-Length: {len(payload)}\r\n\r\n".encode("utf-8")
        self.proc.stdin.write(header + payload)
        self.proc.stdin.flush()
        seq = self.seq
        extra = f" args={json.dumps(arguments)}" if arguments else ""
        print(f"  --> [{seq}] {command}{extra}")
        self.seq += 1
        return seq

    def _read_exact(self, n: int) -> bytes:
        while len(self._read_buf) < n:
            chunk = self.proc.stdout.read(n - len(self._read_buf))
            if not chunk:
                raise EOFError("codelldb stdout fechado")
            self._read_buf += chunk
        data = self._read_buf[:n]
        self._read_buf = self._read_buf[n:]
        return data

    def recv(self) -> dict:
        while b"\r\n\r\n" not in self._read_buf:
            chunk = self.proc.stdout.read(1)
            if not chunk:
                raise EOFError("codelldb stdout fechado durante leitura do header")
            self._read_buf += chunk

        header_bytes, _, self._read_buf = self._read_buf.partition(b"\r\n\r\n")
        header = header_bytes.decode("utf-8")
        length = 0
        for line in header.split("\r\n"):
            if line.startswith("Content-Length: "):
                length = int(line[len("Content-Length: "):])

        if length == 0:
            # codelldb envia Content-Length: 0 como sinal interno (ex: apos loader INT3).
            # Nao e EOF — e uma mensagem vazia que deve ser ignorada.
            return None
        body_bytes = self._read_exact(length)
        msg = json.loads(body_bytes.decode("utf-8", errors="replace"))
        ty = msg.get("type", "?")
        seq = msg.get("seq", "?")
        if ty == "response":
            cmd = msg.get("command", "?")
            ok = msg.get("success", "?")
            body_data = msg.get("body", {})
            extra = f" body={json.dumps(body_data)}" if body_data else ""
            print(f"  <-- [{seq}] response/{cmd} success={ok}{extra}")
        elif ty == "event":
            evt = msg.get("event", "?")
            body_data = msg.get("body", {})
            extra = f" body={json.dumps(body_data)}" if body_data else ""
            print(f"  <-- [{seq}] event/{evt}{extra}")
        else:
            print(f"  <-- [{seq}] {ty}: {json.dumps(msg)}")
        return msg

    def recv_until(self, predicate) -> dict:
        while True:
            msg = self.recv()
            if msg is None:
                continue  # mensagem vazia (Content-Length: 0), ignorar
            if predicate(msg):
                return msg

    def close(self):
        try:
            self.proc.stdin.close()
        except Exception:
            pass
        self.proc.terminate()
        try:
            self.proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def is_response(command: str):
    return lambda m: m.get("type") == "response" and m.get("command") == command


def is_event(name: str):
    return lambda m: m.get("type") == "event" and m.get("event") == name


def is_stopped_or_terminated():
    return lambda m: (
        m.get("type") == "event" and m.get("event") in ("stopped", "terminated", "exited")
    )


def is_terminated():
    return lambda m: (
        m.get("type") == "event" and m.get("event") in ("terminated", "exited")
    )


def main():
    exe = sys.argv[1] if len(sys.argv) >= 2 else r"C:\workspace\rdc_app\target\debug\debug-target-example.exe"

    if not Path(exe).exists():
        print(f"ERRO: executavel nao encontrado: {exe}")
        sys.exit(1)

    src_path = str(SRC_FILE.resolve())
    print(f"Source file: {src_path}")
    print(f"Breakpoint: linha {BREAKPOINT_LINE}")

    codelldb_path = find_codelldb()
    print(f"Usando codelldb: {codelldb_path}")
    print(f"Debugando: {exe}\n")

    proc = subprocess.Popen(
        [str(codelldb_path)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=str(codelldb_path.parent.parent.parent),  # extension/
    )

    def drain_stderr():
        while True:
            data = proc.stderr.readline()
            if not data:
                break
            line = data.decode("utf-8", errors="replace").rstrip()
            if line:
                print(f"  [stderr] {line}")

    threading.Thread(target=drain_stderr, daemon=True).start()

    session = DapSession(proc)

    try:
        # 1. initialize
        print("=== 1. initialize")
        session.send("initialize", {
            "clientID": "rdc-test",
            "clientName": "RDC Test",
            "adapterID": "lldb",
            "linesStartAt1": True,
            "columnsStartAt1": True,
            "supportsRunInTerminalRequest": False,
            "supportsConfigurationDoneRequest": True,
        })
        resp = session.recv_until(is_response("initialize"))
        assert resp.get("success"), f"initialize falhou: {resp}"
        print("  OK initialize\n")

        # 2. launch (sem esperar response - o adapter vai emitir "initialized" primeiro)
        print("=== 2. launch")
        session.send("launch", {
            "name": "debug",
            "type": "lldb",
            "request": "launch",
            "program": exe,
            "args": [],
            "stopOnEntry": False,
            "sourceLanguages": ["rust"],
            "breakpointMode": "file",
        })

        # 3. Aguardar evento "initialized" (adapter pronto para receber config)
        print("=== 3. aguardando evento initialized...")
        session.recv_until(is_event("initialized"))
        print("  OK adapter pronto para configuracao\n")

        # 4. setBreakpoints - AGORA o adapter esta pronto
        print("=== 4. setBreakpoints")
        session.send("setBreakpoints", {
            "source": {
                "name": "main.rs",
                "path": src_path,
            },
            "breakpoints": [{"line": BREAKPOINT_LINE}],
            "lines": [BREAKPOINT_LINE],
        })
        resp = session.recv_until(is_response("setBreakpoints"))
        assert resp.get("success"), f"setBreakpoints falhou: {resp}"
        bps = resp.get("body", {}).get("breakpoints", [])
        for bp in bps:
            verified = bp.get("verified", False)
            msg_text = bp.get("message", "")
            loc = f"line {bp.get('line', '?')}"
            status = "VERIFIED" if verified else "UNVERIFIED"
            print(f"  breakpoint [{status}]: {loc} {msg_text}")
        print()

        # 5. setExceptionBreakpoints
        print("=== 5. setExceptionBreakpoints")
        session.send("setExceptionBreakpoints", {
            "filters": [],
        })
        session.recv_until(is_response("setExceptionBreakpoints"))
        print("  OK\n")

        # 6. configurationDone — a response/launch pode chegar ANTES da response/configurationDone,
        #    entao coletamos ambas aqui sem descartar nenhuma.
        print("=== 6. configurationDone")
        session.send("configurationDone")
        got_launch_resp = False
        got_config_resp = False
        while not (got_launch_resp and got_config_resp):
            msg = session.recv()
            if msg is None:
                continue
            if msg.get("type") == "response" and msg.get("command") == "launch":
                got_launch_resp = True
                if not msg.get("success", True):
                    print(f"  ERRO no launch: {msg}")
                    sys.exit(1)
                print("  OK launch response")
            elif msg.get("type") == "response" and msg.get("command") == "configurationDone":
                got_config_resp = True
                print("  OK configurationDone response")
            # outros eventos (output, module, etc.) sao descartados aqui
        print()

        # 7. Aguardar stopped — sem stopOnEntry, esperamos direto pelo breakpoint.
        #    Ainda pode haver stops automaticos (ex: INT3 do loader do Windows) que ignoramos.
        print("=== 7. aguardando breakpoint na linha {}...".format(BREAKPOINT_LINE))
        thread_id = 1
        while True:
            msg = session.recv_until(is_stopped_or_terminated())
            if is_terminated()(msg):
                print("  ERRO: processo terminou sem parar no breakpoint")
                sys.exit(1)
            body = msg.get("body", {})
            reason = body.get("reason", "?")
            thread_id = body.get("threadId", thread_id)
            print(f"  STOPPED reason={reason} threadId={thread_id}")
            if reason in ("breakpoint", "entry", "step", "pause"):
                print("  stop intencional, prosseguindo\n")
                break
            else:
                # Exception automatica (loader INT3, etc.) — continua
                print(f"  stop automatico ({reason}), continuando...")
                session.send("continue", {"threadId": thread_id})
                session.recv_until(is_response("continue"))

        # 8. stackTrace no breakpoint
        print("=== 8. stackTrace")
        session.send("stackTrace", {"threadId": thread_id, "startFrame": 0, "levels": 10})
        resp = session.recv_until(is_response("stackTrace"))
        frames = resp.get("body", {}).get("stackFrames", [])
        print(f"  {len(frames)} frame(s):")
        for f in frames:
            print(f"    {f.get('name','?')} @ {f.get('source', {}).get('path','?')}:{f.get('line','?')}")
        print()

        # 9. continue final
        print("=== 9. continue (final)")
        session.send("continue", {"threadId": thread_id})
        session.recv_until(is_response("continue"))

        print("=== aguardando terminated...")
        session.recv_until(is_terminated())
        print("  processo terminou\n")

        print("SUCCESS: Fluxo DAP completo funcionou!")

    except Exception as e:
        print(f"\nERRO: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
    finally:
        session.close()


if __name__ == "__main__":
    main()
