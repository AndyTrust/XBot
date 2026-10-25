# Server Google accanto a XBot (Gmail, Drive, Calendar)

*Scritto il 2026-10-25 09:20 da costruttore-xbot (Fase 2.K). Provato sul Mac con colima e un client Google finto; sul server lo mette chi amministra XBot.*

Il server è `workspace-mcp` 2.1.0 (taylorwilsdon/google_workspace_mcp, licenza MIT, da PyPI), in modo OAuth 2.1 su Streamable HTTP. Fa da server di autorizzazione davanti a Google: XBot si registra da solo presso di lui (registrazione dinamica), l'utente dà il consenso due volte di seguito (pagina del server Google, poi Google), il server tiene i token di Google e dà a XBot i suoi. Il client Google dell'utente va nel file `google-mcp.env`, non in XBot.

## File

| File | Cosa |
|---|---|
| `Dockerfile` | `python:3.12-slim` + `pip install "workspace-mcp[disk]==2.1.0"`, utente `app`, dati in `/dati` |
| `compose.yaml` | il container `xbot-google-mcp` sulla rete di `spj-caddy` (`XBOT_RETE_CADDY`), porta `127.0.0.1:18851` solo per le prove |
| `google-mcp.env.esempio` | le variabili, senza valori; la copia vera è `google-mcp.env` (0600, esclusa da git) |
| `caddy-google-mcp.txt` | il blocco da mettere dentro il sito di XBot nel Caddyfile |

## Passi sul server

1. L'utente crea il client: console.cloud.google.com → progetto → abilita Gmail API, Google Drive API, Google Calendar API → schermata di consenso (Esterno, suo indirizzo fra gli utenti di prova) → Credenziali → ID client OAuth → **Applicazione web** → URI di reindirizzamento autorizzato **uno solo**: `https://<dominio>/google-mcp/oauth2callback`. Le stesse istruzioni sono in `client_istruzioni` della voce Google del catalogo.
2. Copia la cartella in `/opt/xbot/google-mcp/`, `cp google-mcp.env.esempio google-mcp.env && chmod 600 google-mcp.env`, e riempi:
   - `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET` dal passo 1;
   - `WORKSPACE_EXTERNAL_URL=https://<dominio>/google-mcp`;
   - `WORKSPACE_MCP_ALLOWED_HOSTS=<dominio>`;
   - `WORKSPACE_MCP_ALLOWED_CLIENT_REDIRECT_URIS=https://<dominio>/api/v1/oauth/ritorno`;
   - `FASTMCP_SERVER_AUTH_GOOGLE_JWT_SIGNING_KEY`: 32 caratteri casuali (`openssl rand -base64 32`).
   - **Non** mettere `GOOGLE_OAUTH_REDIRECT_URI` (vedi «Cosa succede davvero»).
3. La rete di Caddy: `docker inspect spj-caddy --format '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}'`.
4. `XBOT_RETE_CADDY=<rete> docker compose up -d --build`, poi `curl -s http://127.0.0.1:18851/health` → `{"status":"healthy",…,"version":"2.1.0"}`.
5. Il blocco di `caddy-google-mcp.txt` dentro il sito `<dominio>`, prima del `reverse_proxy` di XBot; `caddy validate`, poi `caddy reload`.
6. Controlli da fuori: `curl -s https://<dominio>/.well-known/oauth-protected-resource/google-mcp/mcp` (JSON con `authorization_servers`), `curl -s https://<dominio>/.well-known/oauth-authorization-server/google-mcp` (JSON con `registration_endpoint`), `POST …/google-mcp/mcp` senza token → 401 con `resource_metadata`.
7. In XBot: Collegamenti → catalogo → «Google (Gmail, Drive, Calendar)» → «Collega». L'utente vede la pagina «Application Access Request» del server Google (approva «XBot»), poi Google.

Aggiornare la versione: cambiare `WORKSPACE_MCP_VERSIONE` in `compose.yaml` e `Dockerfile` solo dopo averla provata come sotto.

## Cosa succede davvero (prova sul Mac, 2026-10-25 09:00–09:15)

Container da `docker build` con colima, client Google finto (`finto-123.apps.googleusercontent.com`), `caddy_finto.py` sulla porta 8860 con le stesse tre regole del blocco Caddy, XBot in prova con `XBOT_URL_PUBBLICO=http://127.0.0.1:8860` (`cargo test -p xbot-server --test s2k_collegamenti google -- --ignored --nocapture`).

- `/health` → 200 `{"status":"healthy","service":"workspace-mcp","version":"2.1.0"}`.
- `POST /mcp` senza token → 401 con `WWW-Authenticate: Bearer scope="…" resource_metadata="<base>/.well-known/oauth-protected-resource/google-mcp/mcp"`: i metadati stanno **alla radice del sito** con il prefisso nel percorso, non sotto `/google-mcp/`. Da qui la regola 1 del blocco.
- `GET /.well-known/oauth-authorization-server` del container → `issuer <base>/google-mcp`, `authorization_endpoint …/google-mcp/authorize`, `token_endpoint …/token`, `registration_endpoint …/register`, `token_endpoint_auth_methods_supported ["none","private_key_jwt"]`, `code_challenge_methods_supported ["S256"]`. Il client di XBot (rmcp) lo cerca in `<base>/.well-known/oauth-authorization-server/google-mcp` (RFC 8414): da qui la regola 2 con `rewrite`.
- `POST /collegamenti/{id}/prova` → `{ok: false, errore: "serve il collegamento dell'account"}`.
- `POST /collegamenti/{id}/oauth/avvia` → registrazione dinamica (`POST /google-mcp/register`, nessun client a mano) e `url = <base>/google-mcp/authorize?…&client_id=<uuid del proxy>&redirect_uri=<base>/api/v1/oauth/ritorno&code_challenge_method=S256&scope=<15 scope Google>&resource=<base>/google-mcp/mcp`. L'URL va al **proxy**, non a Google.
- `GET` di quell'URL → 302 a `/google-mcp/consent?txn_id=…`: pagina «Application Access Request: The application XBot wants to access the MCP server google_workspace», con l'indirizzo di ritorno di XBot. «Approve» → 302 a `https://accounts.google.com/o/oauth2/v2/auth?client_id=<client dell'utente>&redirect_uri=<base>/google-mcp/oauth2callback&access_type=offline&prompt=consent&code_challenge_method=S256…`.
- Con `GOOGLE_OAUTH_REDIRECT_URI=<base>/google-mcp/oauth2callback` il ritorno mandato a Google era `<base>/google-mcp/google-mcp/oauth2callback` (prefisso doppio) e il container ascoltava su `/google-mcp/oauth2callback`: per questo la variabile non si mette.
- `WORKSPACE_MCP_ALLOWED_CLIENT_REDIRECT_URIS` limita la registrazione dinamica al ritorno di XBot (il log lo dice all'avvio).
- Non provato: il consenso vero di Google e lo scambio del codice (servono il client dell'utente e il dominio vero), gli strumenti dopo l'accesso.
