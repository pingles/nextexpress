import sys, socket, time; sys.path.insert(0, "/tmp")
from ae_session import connect_until_node
from bbsdrive import render
log=[]
def rec(label, data):
    log.append("\n@@@@@ %s @@@@@"%label); log.append("--RENDER--"); log.append(render(data))
    log.append("--REPR--"); log.append(repr(data)); open("/tmp/cmp/ae_read2.txt","w").write("\n".join(log))

def advance(bbs, target, maxwait=90):
    """Read until target appears, auto-answering (Pause) gates with space."""
    tb = target.encode() if isinstance(target,str) else target
    acc=bytearray(); start=time.time(); last_pause=0; bbs.sock.settimeout(0.3)
    while time.time()-start < maxwait:
        try:
            d=bbs.sock.recv(4096)
            if d==b"": break
            acc+=d; bbs.all_raw+=d
        except socket.timeout:
            pass
        low=bytes(acc).lower()
        if tb.lower() in low: break
        if (b"resume" in low[-180:] or b"(pause)" in low[-180:]) and (time.time()-last_pause>0.8):
            bbs.send(b" "); last_pause=time.time()
    return bytes(acc)

bbs,_=connect_until_node("127.0.0.1",6023,retries=60,log=log)
try:
    bbs.send(b"A\r"); advance(bbs,"Name:",70)
    bbs.send(b"sysop\r"); advance(bbs,"assword",30)
    bbs.send(b"sysop\r"); rec("login->menu", advance(bbs,"mins. left): ",120))
    bbs.send(b"X\r"); rec("X expert", advance(bbs,"mins. left): ",30))
    bbs.send(b"R 1\r"); rec("R 1 (msg1 header+body+subprompt)", advance(bbs,"Options:",40))
    bbs.send(b"\r"); rec("sub <CR> -> msg2", advance(bbs,"Options:",30))
    bbs.send(b"Q\r"); rec("sub Q -> menu", advance(bbs,"mins. left): ",30))
    bbs.send(b"R 2\r"); rec("R 2 (msg2 public header+body)", advance(bbs,"Options:",40))
    bbs.send(b"Q\r"); rec("sub Q -> menu", advance(bbs,"mins. left): ",30))
finally:
    for esc in (b" ",b"\r",b"Q\r",b"\r"):
        try: bbs.send(esc); bbs.read_idle(idle=1.0,maxwait=6)
        except OSError: break
    try:
        bbs.send(b"G Y\r")
        for _ in range(6):
            cc=bbs.read_idle(idle=1.5,maxwait=12); log.append("\n@@logoff@@"); log.append(render(cc))
            if cc==b"" or b"carrier" in cc.lower() or b"click" in cc.lower(): break
    except OSError: pass
    bbs.close(); open("/tmp/cmp/ae_read2.txt","w").write("\n".join(log))
print("AE READ2 DONE")
