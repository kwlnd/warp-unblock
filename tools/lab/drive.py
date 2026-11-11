"""Драйвер стенда: гоняет стратегии через lab.sh на телефоне и пишет результаты в JSONL.

python drive.py catalog            — текущие профили из strategies.json
python drive.py matrix FILE.json    — свой список: [{"name","endpoint","port","jc","jmin","jmax","zapret":[...]}]
Опции: --hold N (секунд удержания после ответа), --repeat N (повторов каждой), --out FILE
"""
import json, subprocess, sys, time, argparse, os

ADB = r"D:\Files\projects\warp-unblock\android\.toolchain\sdk\platform-tools\adb.exe"
FAKE = "/data/local/tmp/wlab/fake"


def run(s, hold):
    args = [str(s["endpoint"]), str(s["port"]), str(s["jc"]), str(s["jmin"]), str(s["jmax"]), str(hold)]
    args += [a.replace("{zapret}", FAKE) for a in s.get("zapret", [])]
    quoted = " ".join("'" + a.replace("'", "") + "'" for a in args)
    cmd = [ADB, "shell", f"su -c \"sh /data/local/tmp/wlab/lab.sh {quoted}\""]
    t = time.time()
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=60 + hold, env={**os.environ, "MSYS_NO_PATHCONV": "1"}).stdout.strip()
    except subprocess.TimeoutExpired:
        out = "FAIL timeout"
    line = out.splitlines()[-1] if out else "FAIL empty"
    return line, round(time.time() - t, 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode")
    ap.add_argument("file", nargs="?")
    ap.add_argument("--hold", type=int, default=0)
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--out", default="results.jsonl")
    a = ap.parse_args()
    if a.mode == "catalog":
        # Каталог: все пары «профиль × адрес из пула».
        here = os.path.dirname(os.path.abspath(__file__))
        cat = json.load(open(os.path.join(here, "../../windows/app/config/strategies.json"), encoding="utf-8"))
        items = []
        for prof in cat["profiles"]:
            for address in cat["endpoints"]:
                ip, port = address.split(":")
                items.append({**prof, "name": f"{prof['name']} · {address}", "endpoint": ip, "port": int(port)})
    else:
        items = json.load(open(a.file, encoding="utf-8"))
    with open(a.out, "a", encoding="utf-8") as f:
        for s in items:
            for r in range(a.repeat):
                line, sec = run(s, a.hold)
                rec = {**s, "result": line, "sec": sec, "hold": a.hold, "t": time.strftime("%H:%M:%S")}
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                f.flush()
                print(f"{line:<22} {sec:>5}s  {s['name']}", flush=True)


if __name__ == "__main__":
    main()
