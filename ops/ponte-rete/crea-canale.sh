#!/bin/bash
# 2026-10-25: crea in XBot il canale «rete-reason» (invia_a_assistente, elenco_assistenti) per il ponte
# della rete di Reason; assistente predefinito: XBOT_ASSISTENTE_PONTE (se manca, XBOT_NOME o «XBot»). Il token va in /home/reason/xbot/canale.txt
# (0600, utente reason), mai stampato. Si lancia sulla VPS da root; sulla VPS di oggi XBOT_ASSISTENTE_PONTE è il nome dell'orchestratore.
set -e
U=${XBOT_URL:?imposta XBOT_URL, es. https://<dominio>}/api/v1
OUT=/home/reason/xbot/canale.txt
PW=$(grep "^Password:" /var/lib/xbot/accesso-iniziale.txt | cut -d" " -f2)
SEG=$(grep -o "secret=[A-Z2-7]*" /var/lib/xbot/totp.txt | head -1 | cut -d= -f2)
totp() { python3 - "$SEG" <<'PY'
import sys,base64,hmac,hashlib,struct,time
s=sys.argv[1]; k=base64.b32decode(s+"="*((8-len(s)%8)%8)); t=int(time.time())//30
h=hmac.new(k,struct.pack(">Q",t),hashlib.sha1).digest(); o=h[-1]&15
print("%06d"%((struct.unpack(">I",h[o:o+4])[0]&0x7fffffff)%1000000))
PY
}
J=$(mktemp); trap "rm -f $J" EXIT
H="Content-Type: application/json"
j() { python3 -c "import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1]))" "$1"; }
mkdir -p /home/reason/xbot; chown reason:reason /home/reason/xbot; chmod 700 /home/reason/xbot
if [ -s "$OUT" ]; then echo "token già presente in $OUT: non ricreo"; exit 0; fi
echo "accesso: $(curl -s -c $J -o /dev/null -w "%{http_code}" -H "$H" -d "{\"password\":\"$PW\",\"codice_totp\":\"$(totp)\",\"dispositivo\":\"canale rete-reason\"}" $U/accesso)"
NOME_PONTE="${XBOT_ASSISTENTE_PONTE:-${XBOT_NOME:-XBot}}"
PREDEFINITO=$(sqlite3 /var/lib/xbot/xbot.sqlite "select id from assistente where nome='${NOME_PONTE//\'/\'\'}' and archiviato_ms is null limit 1")
R=$(curl -s -b $J -H "$H" -d "{\"nome\":\"rete-reason\",\"assistente_id\":\"$PREDEFINITO\",\"strumenti\":[\"invia_a_assistente\",\"elenco_assistenti\"]}" $U/canali)
( umask 077; echo "$R" | j "d['token']" > "$OUT" ); chown reason:reason "$OUT"; chmod 600 "$OUT"
echo "canale: $(echo "$R" | j "d['id']") url $(echo "$R" | j "d.get('url')") assistente «$NOME_PONTE» $PREDEFINITO"
echo "esci: $(curl -s -b $J -X DELETE -o /dev/null -w "%{http_code}" $U/accesso)"
